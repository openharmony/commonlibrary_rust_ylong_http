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

#![cfg(all(feature = "async", feature = "tokio_base", feature = "__tls"))]

mod common;

use std::convert::Infallible;

use ylong_http::response::status::StatusCode;
use ylong_http_client::async_impl::{Body, Client, Request};
use ylong_http_client::{CertVerifier, ServerCerts};

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
#[cfg(feature = "http1_1")]
fn sdv_client_request_http1_verify_true() {
    struct Verifier;
    impl CertVerifier for Verifier {
        fn verify(&self, _certs: &ServerCerts) -> bool {
            true
        }
    }

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
        .cert_verifier(Verifier)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    });
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_http2_verify_true() {
    struct Verifier;
    impl CertVerifier for Verifier {
        fn verify(&self, _certs: &ServerCerts) -> bool {
            true
        }
    }

    define_service_handle!(HTTPS;);

    let rt = init_test_work_runtime(4);

    let key_path = std::path::PathBuf::from("tests/file/key.pem");
    let cert_path = std::path::PathBuf::from("tests/file/cert.pem");

    let (tx, rx) = std::sync::mpsc::channel();
    let server_handle = rt.spawn(async move {
        let handle = {
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
                let mut acceptor =
                    openssl::ssl::SslAcceptor::mozilla_intermediate(openssl::ssl::SslMethod::tls())
                        .expect("SslAcceptorBuilder error");
                acceptor
                    .set_session_id_context(b"test")
                    .expect("Set session id error");
                acceptor
                    .set_private_key_file(key_path, openssl::ssl::SslFiletype::PEM)
                    .expect("Set private key error");
                acceptor
                    .set_certificate_chain_file(cert_path)
                    .expect("Set cert error");
                acceptor.set_alpn_protos(b"\x02h2").unwrap();
                acceptor.set_alpn_select_callback(|_, client| {
                    openssl::ssl::select_next_proto(b"\x02h2", client)
                        .ok_or(openssl::ssl::AlpnError::NOACK)
                });

                let acceptor = acceptor.build();

                let (stream, _) = listener.accept().await.expect("TCP listener accept error");
                let ssl = openssl::ssl::Ssl::new(acceptor.context()).expect("Ssl Error");
                let mut stream =
                    tokio_openssl::SslStream::new(ssl, stream).expect("SslStream Error");
                core::pin::Pin::new(&mut stream).accept().await.unwrap();

                hyper::server::conn::Http::new()
                    .serve_connection(stream, hyper::service::service_fn(server_fn))
                    .await
            });

            TlsHandle { port }
        };
        tx.send(handle)
            .expect("Failed to send the handle to the test thread.");
    });
    rt.block_on(server_handle)
        .expect("Runtime start server coroutine failed");
    let handle = rx
        .recv()
        .expect("Handle send channel (Server-Half) be closed unexpectedly");

    let client = Client::builder()
        .http2_prior_knowledge()
        .cert_verifier(Verifier)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/2.0")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    });
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_http1_verify_false() {
    struct Verifier;
    impl CertVerifier for Verifier {
        fn verify(&self, _certs: &ServerCerts) -> bool {
            false
        }
    }

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
        .cert_verifier(Verifier)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());
    });
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_http1_verify_true_invalid_cert() {
    struct Verifier;
    impl CertVerifier for Verifier {
        fn verify(&self, _certs: &ServerCerts) -> bool {
            true
        }
    }

    define_service_handle!(HTTPS;);

    let rt = init_test_work_runtime(4);

    let key_path = std::path::PathBuf::from("tests/file/invalid_key.pem");
    let cert_path = std::path::PathBuf::from("tests/file/invalid_cert.pem");

    let (tx, rx) = std::sync::mpsc::channel();
    let server_handle = rt.spawn(async move {
        let handle = start_http_server!(
            HTTPS ;
            server_fn ,
            key_path ,
            cert_path
        );
        tx.send(handle)
            .expect("Failed to send the handle to the test thread.");
    });
    rt.block_on(server_handle)
        .expect("Runtime start server coroutine failed");
    let handle = rx
        .recv()
        .expect("Handle send channel (Server-Half) be closed unexpectedly");

    let client = Client::builder()
        .cert_verifier(Verifier)
        .build()
        .expect("Build Client failed.");

    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");

    rt.block_on(async move {
        let response = client.request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    });
}
