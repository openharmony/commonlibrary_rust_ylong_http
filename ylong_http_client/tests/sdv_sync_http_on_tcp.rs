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

#![cfg(all(feature = "sync", feature = "http1_1", feature = "ylong_base"))]

#[macro_use]
mod tcp_server;

use ylong_http_client::sync_impl::Body;

use crate::tcp_server::{format_header_str, TcpHandle};

/// SDV test cases for `sync::Client`.
///
/// # Brief
/// 1. Creates a runtime to host the server.
/// 2. Creates a server within the runtime coroutine.
/// 3. Creates a sync::Client.
/// 4. The client sends a request to the the server.
/// 5. Verifies the response returned by the server.
/// 6. Shuts down the server.
#[test]
fn sdv_synchronized_client_send_request_to_tcp() {
    // `PUT` request.
    sync_client_test_on_tcp!(
        HTTP;
        Request: {
            Method: "PUT",
            Path: "/data",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Header: "Content-Length", "3",
            Body: "Hi!",
        },
    );
}

/// SDV test cases for `sync::Client`.
///
/// # Brief
/// 1. Creates a runtime to host the server.
/// 2. Creates a server within the runtime coroutine.
/// 3. Creates a sync::Client.
/// 4. The client sends requests to the the server repeatedly.
/// 5. Verifies each response returned by the server.
/// 6. Shuts down the server.
#[test]
fn sdv_synchronized_client_send_request_repeatedly_to_tcp() {
    sync_client_test_on_tcp!(
        HTTP;
        Request: {
            Method: "GET",
            Path: "/data",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 201,
            Version: "HTTP/1.1",
            Header: "Content-Length", "11",
            Body: "METHOD GET!",
        },
        Request: {
            Method: "POST",
            Path: "/data",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 201,
            Version: "HTTP/1.1",
            Header: "Content-Length", "12",
            Body: "METHOD POST!",
        },
    );
}

/// SDV test cases for `sync::Client`.
///
/// # Brief
/// 1. Creates a sync::Client.
/// 2. Creates five servers and five client thread sequentially.
/// 3. The client sends requests to the created servers in five thread.
/// 4. Verifies the responses returned by each server.
/// 5. Shuts down the servers.
#[test]
fn sdv_client_making_multiple_connections() {
    sync_client_test_on_tcp!(
        HTTP;
        ClientNum: 5,
        Request: {
            Method: "GET",
            Path: "/data",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 201,
            Version: "HTTP/1.1",
            Header: "Content-Length", "11",
            Body: "METHOD GET!",
        },
    );
}
