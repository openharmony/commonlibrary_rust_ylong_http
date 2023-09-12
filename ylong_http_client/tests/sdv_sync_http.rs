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

#![cfg(all(feature = "sync", feature = "http1_1", feature = "tokio_base"))]

#[macro_use]
mod common;

use std::sync::Arc;

use ylong_http_client::sync_impl::Body;

use crate::common::init_test_work_runtime;

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
fn sdv_synchronized_client_send_request() {
    // `PUT` request.
    sync_client_test_case!(
        HTTP;
        RuntimeThreads: 2,
        Request: {
            Method: "PUT",
            Host: "http://127.0.0.1",
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
fn sdv_synchronized_client_send_request_repeatedly() {
    sync_client_test_case!(
        HTTP;
        RuntimeThreads: 2,
        Request: {
            Method: "GET",
            Host: "http://127.0.0.1",
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
            Host: "http://127.0.0.1",
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
