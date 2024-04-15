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

#![cfg(all(
    feature = "async",
    feature = "http1_1",
    feature = "__c_openssl",
    feature = "tokio_base"
))]

#[macro_use]
mod common;

use std::path::PathBuf;

use ylong_http_client::PubKeyPins;

use crate::common::init_test_work_runtime;

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a hyper https server with the tokio coroutine.
/// 2. Creates an async::Client that with public key pinning.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
#[test]
fn sdv_client_public_key_pinning() {
    define_service_handle!(HTTPS;);
    set_server_fn!(
        ASYNC;
        ylong_server_fn,
        Request: {
            Method: "GET",
            Header: "Content-Length", "5",
            Body: "hello",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Header: "Content-Length", "3",
            Body: "hi!",
        },
    );
    let runtime = init_test_work_runtime(1);
    let mut handles_vec = vec![];
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    {
        start_server!(
            HTTPS;
            ServerNum: 1,
            Runtime: runtime,
            Handles: handles_vec,
            ServeFnName: ylong_server_fn,
        );
        let handle = handles_vec.pop().expect("No more handles !");

        let pins = PubKeyPins::builder()
            .add(
                format!("https://127.0.0.1:{}", handle.port).as_str(),
                "sha256//VHQAbNl67nmkZJNESeTKvTxb5bQmd1maWnMKG/tjcAY=",
            )
            .build()
            .unwrap();

        let client = ylong_http_client::async_impl::Client::builder()
            .tls_ca_file(path.to_str().unwrap())
            .add_public_key_pins(pins)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap();

        let shutdown_handle = runtime.spawn(async move {
            async_client_assertions!(
                ServerHandle: handle,
                ClientRef: client,
                Request: {
                    Method: "GET",
                    Host: "127.0.0.1",
                    Header: "Content-Length", "5",
                    Body: "hello",
                },
                Response: {
                    Status: 200,
                    Version: "HTTP/1.1",
                    Header: "Content-Length", "3",
                    Body: "hi!",
                },
            );
        });
        runtime
            .block_on(shutdown_handle)
            .expect("Runtime block on server shutdown failed");
    }

    {
        start_server!(
            HTTPS;
            ServerNum: 1,
            Runtime: runtime,
            Handles: handles_vec,
            ServeFnName: ylong_server_fn,
        );
        let handle = handles_vec.pop().expect("No more handles !");

        // Two wrong public keys and a correct public key in the middle.
        let pins = PubKeyPins::builder()
            .add(
                format!("https://127.0.0.1:{}", handle.port).as_str(),
                "sha256//YhKJKSzoTt2b5FP18fvpHo7fJYqQCjAa3HWY3tvRMwE=;sha256//VHQAbNl67nmkZJNESeTKvTxb5bQmd1maWnMKG/tjcAY=;sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=",
            )
            .build()
            .unwrap();

        let client = ylong_http_client::async_impl::Client::builder()
            .tls_ca_file(path.to_str().unwrap())
            .add_public_key_pins(pins)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap();

        let shutdown_handle = runtime.spawn(async move {
            async_client_assertions!(
                ServerHandle: handle,
                ClientRef: client,
                Request: {
                    Method: "GET",
                    Host: "127.0.0.1",
                    Header: "Content-Length", "5",
                    Body: "hello",
                },
                Response: {
                    Status: 200,
                    Version: "HTTP/1.1",
                    Header: "Content-Length", "3",
                    Body: "hi!",
                },
            );
        });
        runtime
            .block_on(shutdown_handle)
            .expect("Runtime block on server shutdown failed");
    }

    {
        start_server!(
            HTTPS;
            ServerNum: 1,
            Runtime: runtime,
            Handles: handles_vec,
            ServeFnName: ylong_server_fn,
        );
        let handle = handles_vec.pop().expect("No more handles !");

        // The public key of an irrelevant domain.
        let pins = PubKeyPins::builder()
            .add(
                "https://ylong_http.test:6789",
                "sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=",
            )
            .build()
            .unwrap();

        let client = ylong_http_client::async_impl::Client::builder()
            .tls_ca_file(path.to_str().unwrap())
            .add_public_key_pins(pins)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap();

        let shutdown_handle = runtime.spawn(async move {
            async_client_assertions!(
                ServerHandle: handle,
                ClientRef: client,
                Request: {
                    Method: "GET",
                    Host: "127.0.0.1",
                    Header: "Content-Length", "5",
                    Body: "hello",
                },
                Response: {
                    Status: 200,
                    Version: "HTTP/1.1",
                    Header: "Content-Length", "3",
                    Body: "hi!",
                },
            );
        });
        runtime
            .block_on(shutdown_handle)
            .expect("Runtime block on server shutdown failed");
    }
}

/// SDV test cases for `async::Client`.
///
/// # Brief
/// 1. Starts a hyper https server with the tokio coroutine.
/// 2. Creates an async::Client with an error public key pinning.
/// 3. The client sends a request message.
/// 4. Verifies the received request on the server.
/// 5. The server sends a response message.
/// 6. Verifies the received response on the client.
/// 7. Shuts down the server.
#[test]
fn sdv_client_public_key_pinning_error() {
    define_service_handle!(HTTPS;);
    set_server_fn!(
        ASYNC;
        ylong_server_fn,
        Request: {
            Method: "GET",
            Header: "Content-Length", "5",
            Body: "hello",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Header: "Content-Length", "3",
            Body: "hi!",
        },
    );

    let runtime = init_test_work_runtime(1);

    let mut handles_vec = vec![];
    start_server!(
        HTTPS;
        ServerNum: 1,
        Runtime: runtime,
        Handles: handles_vec,
        ServeFnName: ylong_server_fn,
    );
    let handle = handles_vec.pop().expect("No more handles !");

    let pins = PubKeyPins::builder()
        .add(
            format!("https://127.0.0.1:{}", handle.port).as_str(),
            "sha256//YhKJKSzoTt2b5FP18fvpHo7fJYqQCjAa3HWY3tvRMwE=",
        )
        .build()
        .unwrap();

    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    let client = ylong_http_client::async_impl::Client::builder()
        .tls_ca_file(path.to_str().unwrap())
        .add_public_key_pins(pins)
        .danger_accept_invalid_hostnames(true)
        .build()
        .unwrap();

    let shutdown_handle = runtime.spawn(async move {
        let request = ylong_http_client::async_impl::Request::builder()
            .method("GET")
            .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
            .header("Content-Length", "5")
            .body(ylong_http_client::async_impl::Body::slice("hello"))
            .expect("Request build failed");

        let response = client.request(request).await.err();

        assert_eq!(
            format!("{:?}", response.expect("response is not an error")),
            "HttpClientError { ErrorKind: Connect, Cause: Custom { kind: Other, error: SslError { \
             code: SslErrorCode(1), internal: Some(User(VerifyError { ErrorKind: PubKeyPinning, \
             Cause: Pinned public key verification failed. })) } } }"
        );
    });
    runtime
        .block_on(shutdown_handle)
        .expect("Runtime block on server shutdown failed");
}
