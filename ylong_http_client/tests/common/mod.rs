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

#[cfg(feature = "async")]
mod async_utils;

#[cfg(feature = "sync")]
mod sync_utils;

use tokio::runtime::Runtime;

macro_rules! define_service_handle {
    (
        HTTP;
    ) => {
        use tokio::sync::mpsc::{Receiver, Sender};

        pub struct HttpHandle {
            pub port: u16,

            // This channel allows the server to notify the client when it is up and running.
            pub server_start: Receiver<()>,

            // This channel allows the client to notify the server when it is ready to shut down.
            pub client_shutdown: Sender<()>,

            // This channel allows the server to notify the client when it has shut down.
            pub server_shutdown: Receiver<()>,
        }
    };
    (
        HTTPS;
    ) => {
        pub struct TlsHandle {
            pub port: u16,
        }
    };
}

#[macro_export]
macro_rules! start_server {
    (
        HTTPS;
        ServerNum: $server_num: expr,
        Runtime: $runtime: expr,
        Handles: $handle_vec: expr,
        ServeFnName: $service_fn: ident,
    ) => {{
        for _i in 0..$server_num {
            let (tx, rx) = std::sync::mpsc::channel();
            let server_handle = $runtime.spawn(async move {
                let handle = start_http_server!(
                    HTTPS;
                    $service_fn
                );
                tx.send(handle)
                    .expect("Failed to send the handle to the test thread.");
            });
            $runtime
                .block_on(server_handle)
                .expect("Runtime start server coroutine failed");
            let handle = rx
                .recv()
                .expect("Handle send channel (Server-Half) be closed unexpectedly");
            $handle_vec.push(handle);
        }
    }};
    (
        HTTP;
        ServerNum: $server_num: expr,
        Runtime: $runtime: expr,
        Handles: $handle_vec: expr,
        ServeFnName: $service_fn: ident,
    ) => {{
        for _i in 0..$server_num {
            let (tx, rx) = std::sync::mpsc::channel();
            let server_handle = $runtime.spawn(async move {
                let mut handle = start_http_server!(
                    HTTP;
                    $service_fn
                );
                handle
                    .server_start
                    .recv()
                    .await
                    .expect("Start channel (Server-Half) be closed unexpectedly");
                tx.send(handle)
                    .expect("Failed to send the handle to the test thread.");
            });
            $runtime
                .block_on(server_handle)
                .expect("Runtime start server coroutine failed");
            let handle = rx
                .recv()
                .expect("Handle send channel (Server-Half) be closed unexpectedly");
            $handle_vec.push(handle);
        }
    }};
}

#[macro_export]
macro_rules! start_http_server {
    (
        HTTP;
        $server_fn: ident
    ) => {{
        use hyper::service::{make_service_fn, service_fn};
        use std::convert::Infallible;
        use tokio::sync::mpsc::channel;

        let (start_tx, start_rx) = channel::<()>(1);
        let (client_tx, mut client_rx) = channel::<()>(1);
        let (server_tx, server_rx) = channel::<()>(1);

        let tcp_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server bind port failed !");
        let addr = tcp_listener.local_addr().expect("get server local address failed!");
        let port = addr.port();
        let server = hyper::Server::from_tcp(tcp_listener).expect("build hyper server from tcp listener failed !");

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
    (
        HTTPS;
        $service_fn: ident
    ) => {{
        let mut port = 10000;
        let listener = loop {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => break listener,
                Err(_) => {
                    port += 1;
                    if port == u16::MAX {
                        port = 10000;
                    }
                    continue;
                }
            }
        };
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut acceptor = openssl::ssl::SslAcceptor::mozilla_intermediate(openssl::ssl::SslMethod::tls())
                .expect("SslAcceptorBuilder error");
            acceptor
                .set_session_id_context(b"test")
                .expect("Set session id error");
            acceptor
                .set_private_key_file("tests/file/key.pem", openssl::ssl::SslFiletype::PEM)
                .expect("Set private key error");
            acceptor
                .set_certificate_chain_file("tests/file/cert.pem")
                .expect("Set cert error");
            acceptor.set_alpn_protos(b"\x08http/1.1").unwrap();
            acceptor.set_alpn_select_callback(|_, client| {
                openssl::ssl::select_next_proto(b"\x08http/1.1", client).ok_or(openssl::ssl::AlpnError::NOACK)
            });

            let acceptor = acceptor.build();

            let (stream, _) = listener.accept().await.expect("TCP listener accept error");
            let ssl = openssl::ssl::Ssl::new(acceptor.context()).expect("Ssl Error");
            let mut stream = tokio_openssl::SslStream::new(ssl, stream).expect("SslStream Error");
            core::pin::Pin::new(&mut stream).accept().await.unwrap(); // SSL negotiation finished successfully

            hyper::server::conn::Http::new()
                .http1_only(true)
                .http1_keep_alive(true)
                .serve_connection(stream, hyper::service::service_fn($service_fn))
                .await
        });

        TlsHandle {
            port,
        }
    }};
}

/// Creates a `Request`.
#[macro_export]
#[cfg(feature = "sync")]
macro_rules! ylong_request {
    (
        Request: {
            Method: $method: expr,
            Host: $host: expr,
            Port: $port: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
    ) => {
        ylong_http::request::RequestBuilder::new()
            .method($method)
            .url(format!("{}:{}", $host, $port).as_str())
            $(.header($req_n, $req_v))*
            .body(ylong_http::body::TextBody::from_bytes($req_body.as_bytes()))
            .expect("Request build failed")
    };
}

/// Creates a `Request`.
#[macro_export]
#[cfg(feature = "async")]
macro_rules! ylong_request {
    (
        Request: {
            Method: $method: expr,
            Host: $host: expr,
            Port: $port: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
    ) => {
        ylong_http_client::async_impl::RequestBuilder::new()
             .method($method)
             .url(format!("{}:{}", $host, $port).as_str())
             $(.header($req_n, $req_v))*
             .body(ylong_http_client::async_impl::Body::slice($req_body.as_bytes()))
             .expect("Request build failed")
    };
}

/// Sets server async function.
#[macro_export]
macro_rules! set_server_fn {
    (
        ASYNC;
        $server_fn_name: ident,
        $(Request: {
            Method: $method: expr,
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
    ) => {
        async fn $server_fn_name(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, std::convert::Infallible> {
            match request.method().as_str() {
                // TODO If there are requests with the same Method, an error will be reported for creating two identical match branches.
                $(
                    $method => {
                        assert_eq!($method, request.method().as_str(), "Assert request method failed");
                        assert_eq!(
                            "/",
                            request.uri().to_string(),
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
                            hyper::Response::builder()
                                .version(hyper::Version::HTTP_11)
                                .status($status)
                                $(.header($resp_n, $resp_v))*
                                .body($resp_body.into())
                                .expect("Build response failed")
                        )
                    },
                )*
                _ => {panic!("Unrecognized METHOD !");},
            }
        }

    };
    (
        SYNC;
        $server_fn_name: ident,
        $(Request: {
            Method: $method: expr,
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
    ) => {
        async fn $server_fn_name(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, std::convert::Infallible> {
            match request.method().as_str() {
                // TODO If there are requests with the same Method, an error will be reported for creating two identical match branches.
                $(
                    $method => {
                        assert_eq!($method, request.method().as_str(), "Assert request method failed");
                        assert_eq!(
                            "/",
                            request.uri().to_string(),
                            "Assert request uri failed",
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
                            hyper::Response::builder()
                                .version(hyper::Version::HTTP_11)
                                .status($status)
                                $(.header($resp_n, $resp_v))*
                                .body($resp_body.into())
                                .expect("Build response failed")
                        )
                    },
                )*
                _ => {panic!("Unrecognized METHOD !");},
            }
        }

    };
}

#[macro_export]
macro_rules! ensure_server_shutdown {
    (ServerHandle: $handle:expr) => {
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
    };
}

pub fn init_test_work_runtime(thread_num: usize) -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(thread_num)
        .enable_all()
        .build()
        .expect("Build runtime failed.")
}
