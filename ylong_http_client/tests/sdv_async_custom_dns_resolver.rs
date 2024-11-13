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

use ylong_http_client::async_impl::{Client, DefaultDnsResolver};

use crate::tcp_server::{format_header_str, TcpHandle};

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a tcp server with the ylong_runtime coroutine.
/// 2. Creates an async::Client with dns resolver set by user.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
#[test]
fn sdv_client_custom_dns_resolver() {
    let mut handles_vec = vec![];

    start_tcp_server!(
        ASYNC;
        Proxy: false,
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
            Status: 200,
            Version: "HTTP/1.1",
            Header: "Content-Length", "3",
            Body: "Hi!",
        },
    );

    let handle = handles_vec.pop().expect("No more handles !");

    let resolver = DefaultDnsResolver::default();
    let client = Client::builder().dns_resolver(resolver).build().unwrap();

    let shutdown_handle = ylong_runtime::spawn(async move {
        async_client_assertions_on_tcp!(
            ServerHandle: handle,
            ClientRef: client,
            Request: {
                Method: "GET",
                Version: "HTTP/1.1",
                Path: "/data",
                Header: "Content-Length", "6",
                Header: "Accept", "*/*",
                Body: "Hello!",
            },
            Response: {
                Status: 200,
                Version: "HTTP/1.1",
                Header: "Content-Length", "3",
                Body: "Hi!",
            },
        );
        handle
            .server_shutdown
            .recv()
            .expect("server send order failed !");
    });
    ylong_runtime::block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
}
