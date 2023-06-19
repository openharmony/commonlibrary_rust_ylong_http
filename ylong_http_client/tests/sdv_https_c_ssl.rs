/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#[macro_use]
mod common;

use common::Handle;
use std::path::PathBuf;

// TODO: Add doc for sdv tests.
#[test]
fn sdv_async_client_send_request() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    // `GET` request
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 1,
        IsAsync: true,
        Request: {
            Method: "GET",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 1,
        IsAsync: true,
        Request: {
            Method: "HEAD",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 1,
        IsAsync: true,
        Request: {
            Method: "POST",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 1,
        IsAsync: true,
        Request: {
            Method: "HEAD",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
            Body: "",
        },
        Response: {
            Status: 200,
            Version: "HTTP/1.1",
            Body: "",
        },
    );

    // `PUT` request.
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 1,
        IsAsync: true,
        Request: {
            Method: "PUT",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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

#[test]
fn sdv_client_send_request_repeatedly() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 2,
        IsAsync: true,
        Request: {
            Method: "GET",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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

#[test]
fn sdv_client_making_multiple_connections() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    ylong_client_test_case!(
    Tls: path.to_str().unwrap(),
        RuntimeThreads: 2,
        ClientNum: 5,
        IsAsync: true,
        Request: {
            Method: "GET",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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

#[test]
fn sdv_synchronized_client_send_request() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    // `PUT` request.
    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 2,
        IsAsync: false,
        Request: {
            Method: "PUT",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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

#[test]
fn sdv_synchronized_client_send_request_repeatedly() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(dir);
    path.push("tests/file/root-ca.pem");

    ylong_client_test_case!(
        Tls: path.to_str().unwrap(),
        RuntimeThreads: 2,
        IsAsync: false,
        Request: {
            Method: "GET",
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
            Host: "127.0.0.1",
            Header: "Host", "127.0.0.1",
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
