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

use crate::tcp_server::{format_header_str, TcpHandle};

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a hyper server acts as proxy with the tokio coroutine.
/// 2. Creates an async::Client.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
#[test]
fn sdv_async_client_send_request() {
    let mut handles_vec = vec![];

    start_tcp_server!(
        ASYNC;
        Proxy: true,
        ServerNum: 1,
        Handles: handles_vec,
        Request: {
            Method: "GET",
            Version: "HTTP/1.1",
            Path: "/data",
            Header: "Content-Length", "6",
            Header: "Accept", "*/*",
            Body: "Hello!",
        },
        Response: {
            Status: 201,
            Version: "HTTP/1.1",
            Header: "Content-Length", "11",
            Body: "METHOD GET!",
        },
    );

    let handle = handles_vec.pop().expect("No more handles !");
    let client = ylong_http_client::async_impl::Client::builder()
        .proxy(
            ylong_http_client::Proxy::http(
                format!("http://{}{}", handle.addr.as_str(), "/data").as_str(),
            )
            .build()
            .expect("Http proxy build failed"),
        )
        .build()
        .expect("Client build failed!");

    let shutdown_handle = ylong_runtime::spawn(async move {
        {
            let request = build_client_request!(
                Request: {
                    Method: "GET",
                    Path: "/data",
                    Addr: handle.addr.as_str(),
                    Header: "Content-Length", "6",
                    Body: "Hello!",
                },
            );

            let mut response = client.request(request).await.expect("Request send failed");

            assert_eq!(
                response.status().as_u16(),
                201,
                "Assert response status code failed"
            );
            assert_eq!(
                response.version().as_str(),
                "HTTP/1.1",
                "Assert response version failed"
            );
            assert_eq!(
                response
                    .headers()
                    .get("Content-Length")
                    .unwrap_or_else(|| panic!(
                        "Get response header \"{}\" failed",
                        "Content-Length"
                    ))
                    .to_string()
                    .unwrap_or_else(|_| panic!(
                        "Convert response header \"{}\"into string failed",
                        "Content-Length"
                    )),
                "11",
                "Assert response header \"{}\" failed",
                "Content-Length",
            );
            let mut buf = [0u8; 4096];
            let mut size = 0;
            loop {
                let read = response
                    .body_mut()
                    .data(&mut buf[size..])
                    .await
                    .expect("Response body read failed");
                if read == 0 {
                    break;
                }
                size += read;
            }
            assert_eq!(
                &buf[..size],
                "METHOD GET!".as_bytes(),
                "Assert response body failed"
            );
        }
        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
}
