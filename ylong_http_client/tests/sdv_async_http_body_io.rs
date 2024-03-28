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

use ylong_http::body::async_impl::Body;
use ylong_http_client::async_impl::Client;

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a http server with the ylong runtime.
/// 2. Creates an async::Client.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a chunked response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
#[test]
fn sdv_body_chunk_and_trailer() {
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
            Header: "Transfer-Encoding", "chunked",
            Header: "Trailer", "Expires",
            Body: "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            Expires: Wed, 21 Oct 2015 07:27:00 GMT\r\n\r\n\
            ",
        },
    );

    let handle = handles.pop().unwrap();
    let client = Client::builder().http1_only().build().unwrap();
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
        let mut resp = client.request(request).await.unwrap();

        let mut buf = [0u8; 32];

        // Read body part
        let read = resp.body_mut().data(&mut buf).await.unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf[..read], b"hello");
        let read = resp.body_mut().data(&mut buf).await.unwrap();
        assert_eq!(read, 12);
        assert_eq!(&buf[..read], b"hello world!");
        let read = resp.body_mut().data(&mut buf).await.unwrap();
        assert_eq!(read, 0);
        assert_eq!(&buf[..read], b"");

        let res = resp.body_mut().trailer().await.unwrap().unwrap();
        assert_eq!(
            res.get("expires").unwrap().to_string().unwrap(),
            "Wed, 21 Oct 2015 07:27:00 GMT".to_string()
        );

        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(handle).unwrap();
}
