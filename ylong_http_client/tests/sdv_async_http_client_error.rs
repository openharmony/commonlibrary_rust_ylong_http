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

#![cfg(all(feature = "async", feature = "http1_1", feature = "ylong_base"))]

#[macro_use]
pub mod tcp_server;

use std::io;

use ylong_http_client::async_impl::Client;
use ylong_http_client::{ErrorKind, HttpClientError, Redirect, Timeout};

/// SDV test cases for `ErrorKind`.
///
/// # Brief
/// 1. assert ErrorKind::as_str.
#[test]
fn sdv_client_error_kind() {
    assert_eq!(ErrorKind::BodyDecode.as_str(), "Body Decode Error");
    assert_eq!(ErrorKind::BodyTransfer.as_str(), "Body Transfer Error");
    assert_eq!(ErrorKind::Build.as_str(), "Build Error");
    assert_eq!(ErrorKind::Connect.as_str(), "Connect Error");
    assert_eq!(
        ErrorKind::ConnectionUpgrade.as_str(),
        "Connection Upgrade Error"
    );
    assert_eq!(ErrorKind::Other.as_str(), "Other Error");
    assert_eq!(ErrorKind::Redirect.as_str(), "Redirect Error");
    assert_eq!(ErrorKind::Request.as_str(), "Request Error");
    assert_eq!(ErrorKind::Timeout.as_str(), "Timeout Error");
    assert_eq!(ErrorKind::UserAborted.as_str(), "User Aborted Error");

    let user_aborted = HttpClientError::user_aborted();
    assert_eq!(
        format!("{:?}", user_aborted),
        "HttpClientError { ErrorKind: UserAborted, Cause: No reason }"
    );
    assert_eq!(format!("{}", user_aborted), "User Aborted Error: No reason");

    assert_eq!(user_aborted.error_kind(), ErrorKind::UserAborted);
    let other = HttpClientError::other(user_aborted);
    assert_eq!(other.error_kind(), ErrorKind::Other);
}

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Creates an async::Client with connect timeout 1s.
/// 2. The client sends a request message that uri is reserved.
#[test]
fn sdv_err_start_connect_timeout() {
    let client = Client::builder()
        .connect_timeout(Timeout::from_secs(1))
        .http1_only()
        .build()
        .unwrap();
    let request = build_client_request!(
        Request: {
            Method: "GET",
            Path: "",
            Addr: "198.18.0.25:80",
            Body: "",
        },
    );
    let handle = ylong_runtime::spawn(async move {
        let resp = client.request(request).await;
        assert_eq!(
            resp.map_err(|e| {
                assert_eq!(
                    format!("{:?}", e),
                    "HttpClientError { ErrorKind: Timeout, Cause: Elapsed }"
                );
                assert_eq!(format!("{}", e), "Timeout Error: elapsed");
                e.error_kind()
            })
            .err(),
            Some(ErrorKind::Timeout)
        );
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a tcp server with the ylong runtime.
/// 2. Creates an async::Client with request timeout 1s.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message 1.5s after receiving the request.
/// 6. Shuts down the server.
#[test]
fn sdv_err_req_timeout() {
    use std::time::Duration;

    use tcp_server::{format_header_str, TcpHandle};
    let mut handles = vec![];

    start_tcp_server!(
        ASYNC;
        Proxy: false,
        ServerNum: 1,
        Handles: handles,
        Request: {
            Method: "GET",
            Version: "HTTP/1.1",
            Path: "/data",
            Header: "Content-Length", "5",
            Header: "Accept", "*/*",
            Body: "HELLO",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Header: "Content-Length", "2",
            Body: "HI",
        },
        Sleep: 1500,
    );

    let handle = handles.pop().unwrap();
    let client = Client::builder()
        .request_timeout(Timeout::from_secs(1))
        .http1_only()
        .build()
        .unwrap();
    let request = build_client_request!(
        Request: {
            Method: "GET",
            Path: "/data",
            Addr: handle.addr.as_str(),
            Header: "Content-Length", "5",
            Body: "HELLO",
        },
    );
    let handle = ylong_runtime::spawn(async move {
        let resp = client.request(request).await;
        assert_eq!(
            resp.map_err(|e| {
                assert_eq!(
                    format!("{:?}", e),
                    "HttpClientError { ErrorKind: Timeout, Cause: Kind(TimedOut) }"
                );
                assert_eq!(format!("{}", e), "Timeout Error: timed out");
                assert_eq!(
                    format!("{:?}", e.io_error().unwrap()),
                    format!("{:?}", &io::Error::from(std::io::ErrorKind::TimedOut))
                );
                e.error_kind()
            })
            .err(),
            Some(ErrorKind::Timeout)
        );
        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a tcp server with the ylong runtime.
/// 2. Creates an async::Client.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message with wrong redirect location uri.
/// 6. Shuts down the server.
#[test]
fn sdv_err_redirect_wrong_location() {
    use tcp_server::{format_header_str, TcpHandle};
    let mut handles = vec![];

    start_tcp_server!(
        ASYNC;
        Proxy: false,
        ServerNum: 1,
        Handles: handles,
        Request: {
            Method: "GET",
            Version: "HTTP/1.1",
            Path: "/data",
            Header: "Content-Length", "5",
            Header: "Accept", "*/*",
            Body: "HELLO",
        },
        Response: {
            Status: 307,
            Version: "HTTP/1.1",
            Header: "Content-Length", "2",
            Header: "Location", "http:///data",
            Body: "HI",
        },
    );

    let handle = handles.pop().unwrap();
    let client = Client::builder()
        .http1_only()
        .redirect(Redirect::default())
        .build()
        .unwrap();
    let request = build_client_request!(
        Request: {
            Method: "GET",
            Path: "/data",
            Addr: handle.addr.as_str(),
            Header: "Content-Length", "5",
            Body: "HELLO",
        },
    );
    let handle = ylong_runtime::spawn(async move {
        let resp = client.request(request).await;
        assert_eq!(resp.map_err(|e| {
            assert_eq!(format!("{:?}", e), "HttpClientError { ErrorKind: Redirect, Cause: Illegal location header in response }");
            assert_eq!(format!("{}", e), "Redirect Error: Illegal location header in response");
            e.error_kind() }).err(), Some(ErrorKind::Redirect));
        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a tcp server with the ylong runtime.
/// 2. Creates an async::Client.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message with wrong Content-Length header.
/// 6. Shuts down the server.
#[test]
fn sdv_err_response_with_wrong_body_length() {
    use tcp_server::{format_header_str, TcpHandle};
    let mut handles = vec![];

    start_tcp_server!(
        ASYNC;
        Proxy: false,
        ServerNum: 1,
        Handles: handles,
        Request: {
            Method: "GET",
            Version: "HTTP/1.1",
            Path: "/data",
            Header: "Content-Length", "5",
            Header: "Accept", "*/*",
            Body: "HELLO",
        },
        Response: {
            Status: 307,
            Version: "HTTP/1.1",
            Header: "Content-Length", "0",
            Body: "HELLO",
        },
    );

    let handle = handles.pop().unwrap();
    let client = Client::builder()
        .http1_only()
        .redirect(Redirect::default())
        .build()
        .unwrap();
    let request = build_client_request!(
        Request: {
            Method: "GET",
            Path: "/data",
            Addr: handle.addr.as_str(),
            Header: "Content-Length", "5",
            Body: "HELLO",
        },
    );
    let handle = ylong_runtime::spawn(async move {
        let resp = client.request(request).await;
        assert_eq!(
            resp.map_err(|e| {
                assert_eq!(
                    format!("{}", e),
                    "Request Error: Body length is 0 but read extra data"
                );
                e.error_kind()
            })
            .err(),
            Some(ErrorKind::Request)
        );
        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(handle).unwrap();
}
