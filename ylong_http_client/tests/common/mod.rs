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

/// Server handle.
pub struct Handle {
    pub port: u16,
}

/// Creates a `Request`.
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
        ylong_http_client::RequestBuilder::new()
            .method($method)
            .url(format!("{}:{}", $host, $port).as_str())
            $(.header($req_n, $req_v))*
            .body(ylong_http::body::TextBody::from_bytes($req_body.as_bytes()))
            .expect("Request build failed")
    };
}

/// Sets server async function.
macro_rules! set_server_fn {
    (
        $server_fn_name: ident,
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
    ) => {
        async fn $server_fn_name(request: hyper::Request<hyper::Body>) -> Result<hyper::Response<hyper::Body>, std::convert::Infallible> {
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

macro_rules! get_handle {
    ($service_fn: ident) => {{
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
            let acceptor = acceptor.build();

            // start_tx
            //     .send(())
            //     .await
            //     .expect("Start channel (Client-Half) be closed unexpectedly");

            let (stream, _) = listener.accept().await.expect("TCP listener accpet error");
            let ssl = openssl::ssl::Ssl::new(acceptor.context()).expect("Ssl Error");
            let mut stream = tokio_openssl::SslStream::new(ssl, stream).expect("SslStream Error");
            core::pin::Pin::new(&mut stream).accept().await.unwrap(); // SSL negotiation finished successfully

            hyper::server::conn::Http::new()
                .http1_only(true)
                .http1_keep_alive(true)
                .serve_connection(stream, hyper::service::service_fn($service_fn))
                .await
        });

        Handle {
            port,
        }
    }};
}

macro_rules! ylong_client_test_case {
    (
        Tls: $tls_config: expr,
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
        set_server_fn!(
            ylong_server_fn,
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
        );

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads($thread_num)
            .enable_all()
            .build()
            .expect("Build runtime failed.");

        // The number of servers may be variable based on the number of servers set by the user.
        // However, cliipy checks that the variable does not need to be variable.
        #[allow(unused_mut, unused_assignments)]
        let mut server_num = 1;
        $(server_num = $client_num;)?

        let mut handles_vec = vec![];
        for _i in 0.. server_num {
            let (tx, rx) = std::sync::mpsc::channel();
            let server_handle = runtime.spawn(async move {
                let handle = get_handle!(ylong_server_fn);
                // handle
                //     .server_start
                //     .recv()
                //     .await
                //     .expect("Start channel (Server-Half) be closed unexpectedly");
                tx.send(handle)
                    .expect("Failed to send the handle to the test thread.");
            });
            runtime
                .block_on(server_handle)
                .expect("Runtime start server coroutine failed");
            let handle = rx
                .recv()
                .expect("Handle send channel (Server-Half) be closed unexpectedly");
            handles_vec.push(handle);
        }

        let mut shut_downs = vec![];
        if $is_async {
            let client = ylong_http_client::async_impl::Client::builder()
                .set_ca_file($tls_config)
                .build()
                .unwrap();
            let client = std::sync::Arc::new(client);
            for _i in 0..server_num {
                let handle = handles_vec.pop().expect("No more handles !");

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
            let client = ylong_http_client::sync_impl::Client::builder()
                .set_ca_file($tls_config)
                .build()
                .unwrap();
            let client = std::sync::Arc::new(client);
            for _i in 0..server_num {
                let handle = handles_vec.pop().expect("No more handles !");
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
        use ylong_http_client::async_impl::Body;
        let client = std::sync::Arc::clone(&$async_client);
        let shutdown_handle = $runtime.spawn(async move {
            $(
                let request = ylong_request!(
                    Request: {
                        Method: $method,
                        Host: $host,
                        Port: $handle.port,
                        $(
                            Header: $req_n, $req_v,
                        )*
                        Body: $req_body,
                    },
                );

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
            // $handle
            //     .client_shutdown
            //     .send(())
            //     .await
            //     .expect("Client channel (Server-Half) be closed unexpectedly");
            // $handle
            //     .server_shutdown
            //     .recv()
            //     .await
            //     .expect("Server channel (Server-Half) be closed unexpectedly");
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
        use ylong_http_client::sync_impl::Body;
        let client = std::sync::Arc::clone(&$sync_client);
        $(
            let request = ylong_request!(
                Request: {
                    Method: $method,
                    Host: $host,
                    Port: $handle.port,
                    $(
                        Header: $req_n, $req_v,
                    )*
                    Body: $req_body,
                },
            );
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
            // $handle
            //     .client_shutdown
            //     .send(())
            //     .await
            //     .expect("Client channel (Server-Half) be closed unexpectedly");
            // $handle
            //     .server_shutdown
            //     .recv()
            //     .await
            //     .expect("Server channel (Server-Half) be closed unexpectedly");
        });
        $shut_downs.push(shutdown_handle);
    }};
}
