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
    feature = "http2",
    feature = "tokio_base",
    not(feature = "__tls")
))]

use std::convert::Infallible;
use std::sync::Arc;

use hyper::body::HttpBody;
use tokio::sync::mpsc::{Receiver, Sender};
use ylong_http::body::async_impl::Body as RespBody;
use ylong_http::response::status::StatusCode;
use ylong_http_client::async_impl::{Body, Client, Request};

pub struct HttpHandle {
    pub port: u16,

    // This channel allows the server to notify the client when it is up and running.
    pub server_start: Receiver<()>,

    // This channel allows the client to notify the server when it is ready to shut down.
    pub client_shutdown: Sender<()>,

    // This channel allows the server to notify the client when it has shut down.
    pub server_shutdown: Receiver<()>,
}

async fn server_fn(
    req: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    let (parts, mut body) = req.into_parts();
    assert_eq!(
        parts.method.to_string(),
        "GET",
        "Assert request method failed"
    );
    assert_eq!(
        format!("{:?}", parts.version),
        "HTTP/2.0",
        "Assert request version failed"
    );

    let mut size = 0;
    loop {
        match body.data().await {
            None => {
                break;
            }
            Some(Ok(bytes)) => {
                size += bytes.len();
            }
            Some(Err(_e)) => {
                panic!("server read request body data occurs error");
            }
        }
    }
    assert_eq!(
        size,
        10 * 1024 * 1024,
        "Assert request body data length failed"
    );

    let body_data = vec![b'q'; 10 * 1024 * 1024];
    let response = hyper::Response::builder()
        .version(hyper::Version::HTTP_2)
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::from(body_data))
        .expect("build hyper response failed");
    Ok(response)
}

#[macro_export]
macro_rules! start_http_server {
    (
        HTTP;
        $server_fn: ident
    ) => {{
        use std::convert::Infallible;

        use hyper::service::{make_service_fn, service_fn};
        use tokio::sync::mpsc::channel;

        let (start_tx, start_rx) = channel::<()>(1);
        let (client_tx, mut client_rx) = channel::<()>(1);
        let (server_tx, server_rx) = channel::<()>(1);

        let tcp_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("server bind port failed !");
        let addr = tcp_listener
            .local_addr()
            .expect("get server local address failed!");
        let port = addr.port();

        let server = hyper::Server::from_tcp(tcp_listener)
            .expect("build hyper server from tcp listener failed !");

        tokio::spawn(async move {
            let make_svc =
                make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn($server_fn)) });
            server
                .serve(make_svc)
                .with_graceful_shutdown(async {
                    start_tx
                        .send(())
                        .await
                        .expect("Start channel (Client-Half) be closed unexpectedly");
                    client_rx
                        .recv()
                        .await
                        .expect("Client channel (Client-Half) be closed unexpectedly");
                })
                .await
                .expect("Start server failed");
            server_tx
                .send(())
                .await
                .expect("Server channel (Client-Half) be closed unexpectedly");
        });

        HttpHandle {
            port,
            server_start: start_rx,
            client_shutdown: client_tx,
            server_shutdown: server_rx,
        }
    }};
}

#[test]
fn sdv_async_h2_client_send_request() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("init runtime failed !");

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

    let body_date = vec![b'q'; 10 * 1024 * 1024];

    let client = Client::builder()
        .http2_prior_knowledge()
        .set_stream_recv_window_size(100 * 1024)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .version("HTTP/2.0")
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .method("GET")
        .body(Body::slice(body_date))
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let mut response = client.request(request).await.expect("get response failed");
        assert_eq!(response.status(), StatusCode::OK);

        let mut buf = [0u8; 4096];
        let mut size = 0;
        loop {
            let read = response
                .body_mut()
                .data(&mut buf[..])
                .await
                .expect("Response body read failed");
            if read == 0 {
                break;
            }
            size += read;
        }
        assert_eq!(
            size,
            10 * 1024 * 1024,
            "Assert response body data length failed"
        );

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
fn sdv_async_h2_client_send_request_concurrently() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Build Runtime failed.");

    let client = Client::builder()
        .http2_prior_knowledge()
        .set_stream_recv_window_size(100 * 1024)
        .build()
        .expect("Build Client failed.");

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

    let client_interface = Arc::new(client);
    let mut shut_downs = vec![];

    for _i in 0..5 {
        let client = client_interface.clone();
        let handle = rt.spawn(async move {
            let body_date = vec![b'q'; 1024 * 1024 * 10];

            let request = Request::builder()
                .version("HTTP/2.0")
                .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
                .method("GET")
                .body(Body::slice(body_date))
                .expect("Client build Request failed.");

            let mut response = client.request(request).await.expect("Get Response failed.");
            let mut buf = [0u8; 4096];
            let mut size = 0;

            loop {
                let read = response
                    .body_mut()
                    .data(&mut buf[..])
                    .await
                    .expect("Response body read failed");
                if read == 0 {
                    break;
                }
                size += read;
            }
            assert_eq!(
                size,
                10 * 1024 * 1024,
                "Assert response body data length failed"
            );
        });

        shut_downs.push(handle);
    }

    for shut_down in shut_downs {
        rt.block_on(shut_down)
            .expect("Runtime wait for server shutdown failed");
    }

    rt.block_on(async move {
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
    });
}
