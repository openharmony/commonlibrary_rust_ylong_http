// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::mpsc::Receiver;

#[cfg(feature = "async")]
mod async_utils;

#[cfg(feature = "sync")]
mod sync_utils;

pub struct TcpHandle {
    pub addr: String,

    // This channel allows the server to notify the client when it has shut down.
    pub server_shutdown: Receiver<()>,
}

pub fn format_header_str(key: &str, value: &str) -> String {
    format!("{}:{}\r\n", key.to_ascii_lowercase(), value)
}

#[macro_export]
macro_rules! start_tcp_server {
    (
        ASYNC;
        Proxy: $proxy: expr,
        ServerNum: $server_num: expr,
        Handles: $handle_vec: expr,
        $(
        Request: {
            Method: $method: expr,
            Version: $req_version: expr,
            Path: $path: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        $(
        Response: {
            Status: $status: expr,
            Version: $resp_version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },
        $(Sleep: $during: expr,)?
        )?
        )*
        $(RequestEnds: $end: expr,)?
        $(Shutdown: $shutdown: expr,)?

    ) => {{
        use std::sync::mpsc::channel;
        use ylong_runtime::net::TcpListener;
        use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};

        for _i in 0..$server_num {
            let (rx, tx) = channel();
            let (rx2, tx2) = channel();

            ylong_runtime::spawn(async move {

                let server = TcpListener::bind("127.0.0.1:0").await.expect("server is failed to bind a address !");
                let addr = server.local_addr().expect("failed to get server address !");
                let handle = TcpHandle {
                    addr: addr.to_string(),
                    server_shutdown: tx,
                };
                rx2.send(handle).expect("send TcpHandle out coroutine failed !");

                let (mut stream, _client) = server.accept().await.expect("failed to build a tcp stream");

                $(
                {
                    let mut buf = [0u8; 4096];

                    let size = stream.read(&mut buf).await.expect("tcp stream read error !");
                    let mut length = 0;
                    let crlf = "\r\n";
                    let request_str = String::from_utf8_lossy(&buf[..size]);

                    let request_line = if $proxy {
                        format!("{} http://{}{} {}{}", $method, addr.to_string().as_str(), $path, $req_version, crlf)
                    } else {
                        format!("{} {} {}{}", $method, $path, $req_version, crlf)
                    };
                    assert!(&buf[..size].starts_with(request_line.as_bytes()), "Incorrect Request-Line!");
                    length += request_line.len();

                    let host = format_header_str("host", addr.to_string().as_str());
                    assert!(request_str.contains(host.as_str()), "Incorrect host header!");
                    length += host.len();

                    $(
                    let header_str = format_header_str($req_n, $req_v);
                    assert!(request_str.contains(header_str.as_str()), "Incorrect {} header!", $req_n);
                    length += header_str.len();
                    )*

                    length += crlf.len();
                    length += $req_body.len();

                    if length > size {
                        let size2 = stream.read(&mut buf).await.expect("tcp stream read error2 !");
                        assert_eq!(&buf[..size2], $req_body.as_bytes());
                        assert_eq!(size + size2, length, "Incorrect total request bytes !");
                    } else {
                        assert_eq!(size, length, "Incorrect total request bytes !");
                    }

                    $(
                    let mut resp_str = String::from(format!("{} {} OK\r\n", $resp_version, $status));
                    $(
                    let header = format_header_str($resp_n, $resp_v);
                    resp_str.push_str(header.as_str());
                    )*
                    resp_str.push_str(crlf);
                    resp_str.push_str($resp_body);
                    $(ylong_runtime::time::sleep(Duration::from_millis($during)).await;)?
                    stream.write_all(resp_str.as_bytes()).await.expect("server write response failed");
                    )?
                }
                )*

                $(
                    stream.shutdown($shutdown).expect("server shutdown failed");
                )?
                rx.send(()).expect("server send order failed !");

            });

            let handle = tx2.recv().expect("recv server handle failed !");

            $handle_vec.push(handle);
        }
    }};

    (
        SYNC;
        ServerNum: $server_num: expr,
        Handles: $handle_vec: expr,
        $(Request: {
            Method: $method: expr,
            Path: $path: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*

    ) => {{
        use std::net::TcpListener;
        use std::io::{Read, Write};
        use std::sync::mpsc::channel;
        use std::time::Duration;

        for _i in 0..$server_num {
            let server = TcpListener::bind("127.0.0.1:0").expect("server is failed to bind a address !");
            let addr = server.local_addr().expect("failed to get server address !");
            let (rx, tx) = channel();

            std::thread::spawn( move || {

                let (mut stream, _client) = server.accept().expect("failed to build a tcp stream");
                stream.set_read_timeout(Some(Duration::from_secs(10))).expect("tcp stream set read time out error !");
                stream.set_write_timeout(Some(Duration::from_secs(10))).expect("tcp stream set write time out error !");

                $(
                {
                    let mut buf = [0u8; 4096];

                    let size = stream.read(&mut buf).expect("tcp stream read error !");
                    let mut length = 0;
                    let crlf = "\r\n";
                    let request_str = String::from_utf8_lossy(&buf[..size]);
                    let request_line = format!("{} {} {}{}", $method, $path, "HTTP/1.1", crlf);
                    assert!(&buf[..size].starts_with(request_line.as_bytes()), "Incorrect Request-Line!");

                    length += request_line.len();

                    let accept = format_header_str("accept", "*/*");
                    assert!(request_str.contains(accept.as_str()), "Incorrect accept header!");
                    length += accept.len();

                    let host = format_header_str("host", addr.to_string().as_str());
                    assert!(request_str.contains(host.as_str()), "Incorrect host header!");
                    length += host.len();

                    $(
                    let header_str = format_header_str($req_n, $req_v);
                    assert!(request_str.contains(header_str.as_str()), "Incorrect {} header!", $req_n);
                    length += header_str.len();
                    )*

                    length += crlf.len();
                    length += $req_body.len();

                    if length > size {
                        let size2 = stream.read(&mut buf).expect("tcp stream read error2 !");
                        assert_eq!(&buf[..size2], $req_body.as_bytes());
                        assert_eq!(size + size2, length, "Incorrect total request bytes !");
                    } else {
                        assert_eq!(size, length, "Incorrect total request bytes !");
                    }

                    let mut resp_str = String::from(format!("{} {} OK\r\n", $version, $status));
                    $(
                    let header = format_header_str($resp_n, $resp_v);
                    resp_str.push_str(header.as_str());
                    )*
                    resp_str.push_str(crlf);
                    resp_str.push_str($resp_body);

                    stream.write_all(resp_str.as_bytes()).expect("server write response failed");
                }
                )*
                rx.send(()).expect("server send order failed !");

            });

            let handle = TcpHandle {
                addr: addr.to_string(),
                server_shutdown: tx,
            };
            $handle_vec.push(handle);
        }

    }}
}

/// Creates a sync `Request`.
#[macro_export]
#[cfg(feature = "sync")]
macro_rules! build_client_request {
    (
        Request: {
            Method: $method: expr,
            Path: $path: expr,
            Addr: $addr: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
    ) => {{
        ylong_http::request::RequestBuilder::new()
            .method($method)
            .url(format!("http://{}{}",$addr, $path).as_str())
            $(.header($req_n, $req_v))*
            .body(ylong_http::body::TextBody::from_bytes($req_body.as_bytes()))
            .expect("Request build failed")
    }};
}

/// Creates a sync `Request`.
#[macro_export]
#[cfg(feature = "async")]
macro_rules! build_client_request {
    (
        Request: {
            Method: $method: expr,
            $(Version: $version: expr,)?
            Path: $path: expr,
            Addr: $addr: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
    ) => {{
        ylong_http_client::async_impl::RequestBuilder::new()
             .method($method)
             $(.version($version))?
             .url(format!("http://{}{}",$addr, $path).as_str())
             $(.header($req_n, $req_v))*
             .body(ylong_http_client::async_impl::Body::slice($req_body.as_bytes()))
             .expect("Request build failed")
    }};
}
