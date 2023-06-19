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

use tokio::sync::mpsc::{Receiver, Sender};

pub struct Handle {
    pub port: u16,

    // This channel allows the server to notify the client when it is up and running.
    pub server_start: Receiver<()>,

    // This channel allows the client to notify the server when it is ready to shut down.
    pub client_shutdown: Sender<()>,

    // This channel allows the server to notify the client when it has shut down.
    pub server_shutdown: Receiver<()>,
}

#[macro_export]
macro_rules! start_local_test_server {
    ($server_fn: ident) => {{
        use hyper::service::{make_service_fn, service_fn};
        use hyper::Server;
        use std::convert::Infallible;
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        use tokio::sync::mpsc::channel;

        let (start_tx, start_rx) = channel::<()>(1);
        let (client_tx, mut client_rx) = channel::<()>(1);
        let (server_tx, server_rx) = channel::<()>(1);
        let mut port = 10000;

        let server = loop {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
            match Server::try_bind(&addr) {
                Ok(server) => break server,
                Err(_) => {
                    port += 1;
                    if port == u16::MAX {
                        port = 10000;
                    }
                    continue;
                }
            }
        };

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

        Handle {
            port,
            server_start: start_rx,
            client_shutdown: client_tx,
            server_shutdown: server_rx,
        }
    }};
}

#[macro_export]
macro_rules! ylong_client_test_case {
        (
            RuntimeThreads: $thread_num: expr,
            $(ClientNum: $client_num: expr,)?
            IsAsync: $is_async: expr,
            $(Request: {
                Method: $method: expr,
                Host: $host: expr,
                $(
                    Header: $req_n: expr, $req_v: expr,
                )*
                Body: $req_body: expr,
            },
            Response: {
                Status: $status: expr,
                Version: $version: expr,
                $(
                    Header: $resp_n: expr, $resp_v: expr,
                )*
                Body: $resp_body: expr,
            },)*
        ) => {{
            async fn server_fn(request: Request<Body>) -> Result<Response<Body>, Infallible> {

                match request.method().as_str() {
                    $(
                        $method => {
                            assert_eq!($method, request.method().as_str(), "Assert request method failed");
                            assert_eq!(
                                $host,
                                request.uri().host().expect("Uri in request do not have a host."),
                                "Assert request host failed",
                            );
                            assert_eq!(
                                $version,
                                format!("{:?}", request.version()),
                                "Assert request version failed",
                            );
                            $(assert_eq!(
                                $req_v,
                                request
                                    .headers()
                                    .get($req_n)
                                    .expect(format!("Get request header \"{}\" failed", $req_n).as_str())
                                    .to_str()
                                    .expect(format!("Convert request header \"{}\" into string failed", $req_n).as_str()),
                                "Assert request header {} failed", $req_n,
                            );)*
                            let body = hyper::body::to_bytes(request.into_body()).await
                                .expect("Get request body failed");
                            assert_eq!($req_body.as_bytes(), body, "Assert request body failed");

                            Ok(
                                Response::builder()
                                    .version(hyper::Version::HTTP_11)
                                    .status($status)
                                    $(.header($resp_n, $resp_v))*
                                    .body($resp_body.into())
                                    .expect("Build response failed")
                            )
                        },
                    )*
                    _ => {panic!("Unrecognized METHOD!");},
                }
            }
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads($thread_num)
                .enable_all()
                .build()
                .expect("Build runtime failed.");

            // The number of servers may vary based on the user's configuration.
            // However, Clippy checks to ensure that this variable doesn't need to be mutable.
            #[allow(unused_mut, unused_assignments)]
            let mut server_num = 1;
            $(server_num = $client_num;)?

            let mut handles_vec = vec![];
            for _i in 0.. server_num {
                let (tx, rx) = std::sync::mpsc::channel();
                let server_handle = runtime.spawn(async move {
                    let mut handle = start_local_test_server!(server_fn);
                    handle.server_start.recv().await.expect("Start channel (Server-Half) be closed unexpectedly");
                    tx.send(handle).expect("Failed to send the handle to the test thread.");
                });
                runtime.block_on(server_handle).expect("Runtime start server coroutine failed");
                let handle = rx.recv().expect("Handle send channel (Server-Half) be closed unexpectedly");
                handles_vec.push(handle);
            }

            let mut shut_downs = vec![];
            if $is_async {
                let client = ylong_http_client::async_impl::Client::new();
                let client = Arc::new(client);
                 for _i in 0..server_num {
                    let mut handle = handles_vec.pop().expect("No more handles !");

                    ylong_client_test_case!(
                        Runtime: runtime,
                        AsyncClient: client,
                        ServerHandle: handle,
                        ShutDownHandles: shut_downs,
                        $(Request: {
                            Method: $method,
                            Host: $host,
                            $(
                                Header: $req_n, $req_v,
                            )*
                            Body: $req_body,
                        },
                        Response: {
                            Status: $status,
                            Version: $version,
                            $(
                                Header: $resp_n, $resp_v,
                            )*
                            Body: $resp_body,
                        },)*
                    )
                }
            } else {
                let client = ylong_http_client::sync_impl::Client::new();
                let client = Arc::new(client);
                for _i in 0..server_num {
                    let mut handle = handles_vec.pop().expect("No more handles !");
                    ylong_client_test_case!(
                        Runtime: runtime,
                        SyncClient: client,
                        ServerHandle: handle,
                        ShutDownHandles: shut_downs,
                        $(Request: {
                            Method: $method,
                            Host: $host,
                            $(
                                Header: $req_n, $req_v,
                            )*
                            Body: $req_body,
                        },
                        Response: {
                            Status: $status,
                            Version: $version,
                            $(
                                Header: $resp_n, $resp_v,
                            )*
                            Body: $resp_body,
                        },)*
                    )
                }
            }
            for shutdown_handle in shut_downs {
                runtime.block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
            }
        }};

    (
        Runtime: $runtime: expr,
        AsyncClient: $async_client: expr,
        ServerHandle: $handle:expr,
        ShutDownHandles: $shut_downs: expr,
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        let client = Arc::clone(&$async_client);
        let shutdown_handle = $runtime.spawn(async move {
            $(
            let request = RequestBuilder::new()
            .method($method)
            .url(format!("{}:{}", $host, $handle.port).as_str())
            $(.header($req_n, $req_v))*
            .body(TextBody::from_bytes($req_body.as_bytes()))
            .expect("Request build failed");
            let mut response = client
            .request(request)
            .await
            .expect("Request send failed");
            assert_eq!(response.status().as_u16(), $status, "Assert response status code failed");
            assert_eq!(response.version().as_str(), $version, "Assert response version failed");
            $(assert_eq!(
                response
                .headers()
                .get($resp_n)
                .expect(format!("Get response header \"{}\" failed", $resp_n).as_str())
                .to_str()
                .expect(format!("Convert response header \"{}\"into string failed", $resp_n).as_str()),
                $resp_v,
                "Assert response header \"{}\" failed", $resp_n,
            );)*
            let mut buf = [0u8; 4096];
            let mut size = 0;
            loop {
                let read = response
                .body_mut()
                .data(&mut buf[size..]).await
                .expect("Response body read failed");
                if read == 0 {
                    break;
                }
                size += read;
            }
            assert_eq!(&buf[..size], $resp_body.as_bytes(), "Assert response body failed");
            )*
            $handle
            .client_shutdown
            .send(())
            .await
            .expect("Client channel (Server-Half) be closed unexpectedly");
            $handle
            .server_shutdown
            .recv()
            .await
            .expect("Server channel (Server-Half) be closed unexpectedly");
        });
        $shut_downs.push(shutdown_handle);
    }};

    (
        Runtime: $runtime: expr,
        SyncClient: $sync_client: expr,
        ServerHandle: $handle:expr,
        ShutDownHandles: $shut_downs: expr,
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        let client = Arc::clone(&$sync_client);
        $(
        let request = RequestBuilder::new()
        .method($method)
        .url(format!("{}:{}", $host, $handle.port).as_str())
        $(.header($req_n, $req_v))*
        .body(TextBody::from_bytes($req_body.as_bytes()))
        .expect("Request build failed");
        let mut response = client
        .request(request)
        .expect("Request send failed");
        assert_eq!(response.status().as_u16(), $status, "Assert response status code failed");
        assert_eq!(response.version().as_str(), $version, "Assert response version failed");
        $(assert_eq!(
            response
            .headers()
            .get($resp_n)
            .expect(format!("Get response header \"{}\" failed", $resp_n).as_str())
            .to_str()
            .expect(format!("Convert response header \"{}\"into string failed", $resp_n).as_str()),
            $resp_v,
            "Assert response header \"{}\" failed", $resp_n,
        );)*
        let mut buf = [0u8; 4096];
        let mut size = 0;
        loop {
            let read = response
            .body_mut()
            .data(&mut buf[size..])
            .expect("Response body read failed");
            if read == 0 {
                break;
            }
            size += read;
        }
        assert_eq!(&buf[..size], $resp_body.as_bytes(), "Assert response body failed");
        )*
        let shutdown_handle = $runtime.spawn(async move {
            $handle.client_shutdown.send(()).await.expect("Client channel (Server-Half) be closed unexpectedly");
            $handle.server_shutdown.recv().await.expect("Server channel (Server-Half) be closed unexpectedly");
        });
        $shut_downs.push(shutdown_handle);
    }}

}
