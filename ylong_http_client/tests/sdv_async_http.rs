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

#![cfg(all(feature = "async", feature = "http1_1", feature = "tokio_base"))]

#[macro_use]
mod common;

use crate::common::init_test_work_runtime;

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a hyper server with the tokio coroutine.
/// 2. Creates an async::Client.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
/// 8. Repeats the preceding operations to start the next test case.
#[test]
fn sdv_async_client_send_request() {
    // `GET` request
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 1,
        Request: {
            Method: "GET",
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

    // `HEAD` request.
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 1,
        Request: {
            Method: "HEAD",
            Host: "http://127.0.0.1",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Body: "",
        },
    );

    // `Post` Request.
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 1,
        Request: {
            Method: "POST",
            Host: "http://127.0.0.1",
            Header: "Content-Length", "6",
            Body: "Hello!",
        },
        Response: {
            Status: 201,
            Version: "HTTP/1.1",
            Header: "Content-Length", "3",
            Body: "Hi!",
        },
    );

    // `HEAD` request without body.
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 1,
        Request: {
            Method: "HEAD",
            Host: "http://127.0.0.1",
            Body: "",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Body: "",
        },
    );

    // `PUT` request.
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 1,
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

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Creates a hyper server with the tokio coroutine.
/// 2. Creates an async::Client.
/// 3. The client repeatedly sends requests to the the server.
/// 4. Verifies each response returned by the server.
/// 5. Shuts down the server.
#[test]
fn sdv_client_send_request_repeatedly() {
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
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

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Creates an async::Client.
/// 2. Creates five servers and five coroutine sequentially.
/// 3. The client sends requests to the created servers in five coroutines.
/// 4. Verifies the responses returned by each server.
/// 5. Shuts down the servers.
#[test]
fn sdv_client_making_multiple_connections() {
    async_client_test_case!(
        HTTP;
        ServeFnName: ylong_server_fn,
        RuntimeThreads: 2,
        ClientNum: 5,
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
    );
}
