// Copyright (c) 2025 Huawei Device Co., Ltd.
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
mod common;
use std::convert::Infallible;

// use tokio::sync::mpsc::{Receiver, Sender};
use ylong_http::response::status::StatusCode;
use ylong_http::version::Version;
use ylong_http_client::async_impl::{Body, Client, Request};

use crate::common::init_test_work_runtime;

#[test]
#[cfg(not(feature = "__tls"))]
fn sdv_async_http10_get() {
    define_service_handle!(HTTP;);

    async fn server_fn(
        _req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        let response = hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::empty())
            .expect("build hyper response failed");
        Ok(response)
    }

    let rt = init_test_work_runtime(4);

    let (tx, rx) = std::sync::mpsc::channel();

    rt.block_on(async move {
        let mut handle = start_http_server!(HTTP; server_fn);
        handle
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        tx.send(handle)
            .expect("send Handle out the server coroutine failed !");
    });

    let mut handle = rx.recv().expect("recv Handle failed !");

    let client = Client::builder().build().expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.0")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await.expect("get response failed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.version(), &Version::HTTP1_0);

        handle
            .client_shutdown
            .send(())
            .await
            .expect("send client shutdown");
        handle
            .server_shutdown
            .recv()
            .await
            .expect("server shutdown");
    })
}

#[test]
#[cfg(feature = "__tls")]
fn sdv_async_https10_get() {
    define_service_handle!(HTTPS;);

    async fn server_fn(
        _req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        let response = hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::empty())
            .expect("build hyper response failed");
        Ok(response)
    }

    let rt = init_test_work_runtime(4);

    let mut handles_vec = vec![];
    start_server!(
        HTTPS;
        ServerNum: 1,
        Runtime: rt,
        Handles: handles_vec,
        ServeFnName: server_fn,
    );
    let handle = handles_vec.pop().expect("No more handles !");

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.0")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await.expect("Request send failed");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.version(), &Version::HTTP1_0);
    });
}

#[test]
#[cfg(not(feature = "__tls"))]
fn sdv_async_http10_connect() {
    define_service_handle!(HTTP;);

    async fn server_fn(
        _req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        let response = hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::empty())
            .expect("build hyper response failed");
        Ok(response)
    }

    let rt = init_test_work_runtime(4);

    let (tx, rx) = std::sync::mpsc::channel();

    rt.block_on(async move {
        let mut handle = start_http_server!(HTTP; server_fn);
        handle
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        tx.send(handle)
            .expect("send Handle out the server coroutine failed !");
    });

    let mut handle = rx.recv().expect("recv Handle failed !");

    let client = Client::builder().build().expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.0")
        .method("CONNECT")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());

        handle
            .client_shutdown
            .send(())
            .await
            .expect("send client shutdown");
        handle
            .server_shutdown
            .recv()
            .await
            .expect("server shutdown");
    })
}

#[test]
#[cfg(not(feature = "__tls"))]
fn sdv_async_http10_no_support() {
    define_service_handle!(HTTP;);

    async fn server_fn(
        req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        if req.version() == hyper::Version::HTTP_10 {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::HTTP_VERSION_NOT_SUPPORTED)
                .body(hyper::Body::empty())
                .unwrap());
        }
        let response = hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::empty())
            .expect("build hyper response failed");
        Ok(response)
    }

    let rt = init_test_work_runtime(4);

    let (tx, rx) = std::sync::mpsc::channel();

    rt.block_on(async move {
        let mut handle = start_http_server!(HTTP; server_fn);
        handle
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        tx.send(handle)
            .expect("send Handle out the server coroutine failed !");
    });

    let mut handle = rx.recv().expect("recv Handle failed !");

    let client = Client::builder().build().expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.0")
        .method("CONNECT")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());

        handle
            .client_shutdown
            .send(())
            .await
            .expect("send client shutdown");
        handle
            .server_shutdown
            .recv()
            .await
            .expect("server shutdown");
    })
}
