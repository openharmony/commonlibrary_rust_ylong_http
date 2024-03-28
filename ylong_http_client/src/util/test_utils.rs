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

//! Construct the http server using TcpStream.

use std::sync::mpsc::Receiver;

pub(crate) struct TcpHandle {
    pub addr: String,

    // This channel allows the server to notify the client when it has shut down.
    pub server_shutdown: Receiver<()>,
}

pub(crate) fn format_header_str(key: &str, value: &str) -> String {
    format!("{}:{}\r\n", key.to_ascii_lowercase(), value)
}

#[macro_export]
macro_rules! build_client_request {
    (
        Request: {
            $(Method: $method: expr,)?
            $(
                Version: $version: expr,
            )?
            Path: $path: expr,
            Addr: $addr: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
    ) => {{
            Request::builder()
                 $(.method($method))?
                 $(.version($version))?
                 .url(format!("http://{}{}",$addr, $path).as_str())
                 $(.header($req_n, $req_v))*
                 .body($req_body)
                 .expect("Request build failed")
        }};
    }

#[macro_export]
macro_rules! start_tcp_server {
    (
        Handles: $handle_vec: expr,
        $(EndWith: $end: expr,)?
        $(
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },
        )?
        $(Shutdown: $shutdown: expr,)?

    ) => {{
            use std::sync::mpsc::channel;
            use ylong_runtime::net::TcpListener;
            use ylong_runtime::io::AsyncReadExt;

            let (tx, rx) = channel();
            let (tx2, rx2) = channel();

                ylong_runtime::spawn(async move {

                    let server = TcpListener::bind("127.0.0.1:0").await.expect("server is failed to bind a address !");
                    let addr = server.local_addr().expect("failed to get server address !");
                    let handle = TcpHandle {
                        addr: addr.to_string(),
                        server_shutdown: rx,
                    };
                    tx2.send(handle).expect("send TcpHandle out coroutine failed !");

                    let (mut stream, _client) = server.accept().await.expect("failed to build a tcp stream");

                    let mut buf = [0u8; 4096];
                    let _size = stream.read(&mut buf).await.expect("tcp stream read error !");

                    $(
                    let mut total = _size;
                    while !&buf[..total].ends_with($end.as_bytes()) {
                        let tmp_size = stream.read(&mut buf[total..]).await.expect("tcp stream read error !");
                        total += tmp_size;
                    }
                    )?
                    $(
                    {
                        let crlf = "\r\n";
                        let mut resp_str = String::from(format!("{} {} OK\r\n", $version, $status));
                        $(
                        let header = format_header_str($resp_n, $resp_v);
                        resp_str.push_str(header.as_str());
                        )*
                        resp_str.push_str(crlf);
                        resp_str.push_str($resp_body);

                        stream.write_all(resp_str.as_bytes()).await.expect("server write response failed");
                    }
                    )?
                    $(
                    stream.shutdown($shutdown).expect("server shutdown failed");
                    )?

                    tx.send(()).expect("server send order failed !");

                });

                let handle = rx2.recv().expect("recv server handle failed !");

                $handle_vec.push(handle);
        }};
    }
