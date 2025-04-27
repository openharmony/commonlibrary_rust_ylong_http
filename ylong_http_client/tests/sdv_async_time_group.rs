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

#![cfg(all(feature = "async", feature = "tokio_base"))]

mod common;

use std::convert::Infallible;
use std::time::Instant;

use ylong_http::response::status::StatusCode;
use ylong_http_client::async_impl::{Body, Client, Request};

use crate::common::init_test_work_runtime;

async fn server_fn(
    _req: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    let response = hyper::Response::builder()
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::empty())
        .expect("build hyper response failed");
    Ok(response)
}

#[test]
#[cfg(all(feature = "http1_1", not(feature = "__tls")))]
fn sdv_client_request_time_group_http1() {
    define_service_handle!(HTTP;);

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
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let start = Instant::now();
        let response = client.request(request).await.expect("get response failed");
        let cost = Instant::now() - start;
        assert_eq!(response.status(), StatusCode::OK);

        let time_group = response.time_group();
        assert!(time_group.dns_duration().unwrap() < cost);
        assert!(time_group.connect_duration().unwrap() < cost);
        assert!(time_group.tcp_duration().unwrap() < cost);
        assert!(time_group.transfer_duration().unwrap() < cost);

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
#[cfg(all(feature = "http1_1", feature = "__tls"))]
fn sdv_client_request_time_group_https1() {
    define_service_handle!(HTTPS;);

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
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let start = Instant::now();
        let response = client.request(request).await.expect("get response failed");
        let cost = Instant::now() - start;
        assert_eq!(response.status(), StatusCode::OK);

        let time_group = response.time_group();
        assert!(time_group.dns_duration().unwrap() < cost);
        assert!(time_group.connect_duration().unwrap() < cost);
        assert!(time_group.tls_duration().unwrap() < cost);
        assert!(time_group.tcp_duration().unwrap() < cost);
        assert!(time_group.transfer_duration().unwrap() < cost);
    });
}

#[test]
#[cfg(all(feature = "http2", not(feature = "__tls")))]
fn sdv_client_request_time_group_http2() {
    define_service_handle!(HTTP;);

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

    let client = Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/2.0")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let start = Instant::now();
        let response = client.request(request).await.expect("get response failed");
        let cost = Instant::now() - start;
        assert_eq!(response.status(), StatusCode::OK);

        let time_group = response.time_group();
        assert!(time_group.dns_duration().unwrap() < cost);
        assert!(time_group.connect_duration().unwrap() < cost);
        assert!(time_group.tcp_duration().unwrap() < cost);
        assert!(time_group.transfer_duration().unwrap() < cost);

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
#[cfg(all(feature = "http1_1", not(feature = "__tls")))]
fn sdv_client_request_time_group_proxy() {
    define_service_handle!(HTTP;);

    let rt = init_test_work_runtime(4);

    let (mut handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!(HTTP; server_fn);
        let mut handle2 = start_http_server!(HTTP; server_fn);
        handle1
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        handle2
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        (handle1, handle2)
    });

    let client = Client::builder()
        .proxy(
            ylong_http_client::Proxy::http(
                format!("http://{}:{}", "127.0.0.1", handle2.port).as_str(),
            )
            .build()
            .unwrap(),
        )
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle1.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let start = Instant::now();
        let response = client.request(request).await.expect("get response failed");
        let cost = Instant::now() - start;
        assert_eq!(response.status(), StatusCode::OK);

        let time_group = response.time_group();
        assert!(time_group.dns_duration().unwrap() < cost);
        assert!(time_group.connect_duration().unwrap() < cost);
        assert!(time_group.tcp_duration().unwrap() < cost);
        assert!(time_group.transfer_duration().unwrap() < cost);

        handle1
            .client_shutdown
            .send(())
            .await
            .expect("send client shutdown");
        handle1
            .server_shutdown
            .recv()
            .await
            .expect("server shutdown");
        handle2
            .client_shutdown
            .send(())
            .await
            .expect("send client shutdown");
        handle2
            .server_shutdown
            .recv()
            .await
            .expect("server shutdown");
    })
}

#[test]
#[cfg(all(feature = "http1_1", not(feature = "__tls")))]
fn sdv_client_request_time_group_redirect() {
    define_service_handle!(HTTP;);

    let rt = init_test_work_runtime(4);

    let mut handle = rt.block_on(async move {
        let mut handle = start_http_server!(HTTP; server_fn);
        handle
            .server_start
            .recv()
            .await
            .expect("recv server start msg failed !");
        handle
    });

    let client = Client::builder()
        .redirect(ylong_http_client::Redirect::default())
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let start = Instant::now();
        let response = client.request(request).await.expect("get response failed");
        let cost = Instant::now() - start;
        assert_eq!(response.status(), StatusCode::OK);

        let time_group = response.time_group();
        assert!(time_group.dns_duration().unwrap() < cost);
        assert!(time_group.connect_duration().unwrap() < cost);
        assert!(time_group.tcp_duration().unwrap() < cost);
        assert!(time_group.transfer_duration().unwrap() < cost);

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
