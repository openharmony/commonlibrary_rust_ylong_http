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

#![cfg(all(
    feature = "async",
    feature = "tokio_base",
    feature = "http1_1",
    not(feature = "__tls")
))]

mod common;

use std::convert::Infallible;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use ylong_http_client::async_impl::{Body, Client, Request, Resolver, SocketFuture};

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

macro_rules! dns_test {
    (
        Ipv4;
        Success;
        $resolver: ident
    ) => {
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
            .dns_resolver($resolver)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("localhost:{}", handle.port).as_str())
            .body(Body::empty())
            .expect("Client build Request failed.");

        rt.block_on(async move {
            let response = client.request(request).await;
            assert!(response.is_ok());

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
    };
    (
        Ipv6;
        Success;
        $resolver: ident
    ) => {
        define_service_handle!(HTTP;);
        let rt = init_test_work_runtime(4);
        let (tx, rx) = std::sync::mpsc::channel();

        rt.block_on(async move {
            let mut handle = start_http_server!(HTTP; Ipv6; server_fn);
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
            .dns_resolver($resolver)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("localhost:{}", handle.port).as_str())
            .body(Body::empty())
            .expect("Client build Request failed.");

        rt.block_on(async move {
            let response = client.request(request).await;
            assert!(response.is_ok());

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
    };
    (
        Ipv4;
        Fail;
        $resolver: ident
    ) => {
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
            .dns_resolver($resolver)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("localhost:{}", handle.port).as_str())
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
    };
    (
        Ipv6;
        Fail;
        $resolver: ident
    ) => {
        define_service_handle!(HTTP;);
        let rt = init_test_work_runtime(4);
        let (tx, rx) = std::sync::mpsc::channel();

        rt.block_on(async move {
            let mut handle = start_http_server!(HTTP; Ipv6; server_fn);
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
            .dns_resolver($resolver)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("localhost:{}", handle.port).as_str())
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
    };
}

#[test]
fn sdv_client_request_dns_resolver_ipv4() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip = Ipv4Addr::new(127, 0, 0, 1);
                    let addr = SocketAddr::from((ip, port.parse().unwrap()));
                    let addrs = vec![addr];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv4; Success; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv6() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
                    let addr = SocketAddr::from((ip, port.parse().unwrap()));
                    let addrs = vec![addr];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv6; Success; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv4_invalid() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip = Ipv4Addr::new(127, 0, 0, 2);
                    let addr = SocketAddr::from((ip, port.parse().unwrap()));
                    let addrs = vec![addr];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv4; Fail; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv6_invalid() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2);
                    let addr = SocketAddr::from((ip, port.parse().unwrap()));
                    let addrs = vec![addr];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv6; Fail; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv4_multiple1() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip1 = Ipv4Addr::new(127, 0, 0, 1);
                    let addr1 = SocketAddr::from((ip1, port.parse().unwrap()));
                    let ip2 = Ipv4Addr::new(127, 0, 0, 2);
                    let addr2 = SocketAddr::from((ip2, port.parse().unwrap()));
                    let addrs = vec![addr1, addr2];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv4; Success; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv6_multiple1() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.trim_start_matches("localhost:").to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let ip1 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
                    let addr1 = SocketAddr::from((ip1, port.parse().unwrap()));
                    let ip2 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2);
                    let addr2 = SocketAddr::from((ip2, port.parse().unwrap()));
                    let addrs = vec![addr1, addr2];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    dns_test!(Ipv6; Success; ExampleDnsResolver);
}

#[test]
fn sdv_client_request_dns_resolver_ipv4_multiple2() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let (port1, port2) = port.split_once(':').unwrap();
                    let ip1 = Ipv4Addr::new(127, 0, 0, 1);
                    let addr1 = SocketAddr::from((ip1, port1.parse().unwrap()));
                    let ip2 = Ipv4Addr::new(127, 0, 0, 1);
                    let addr2 = SocketAddr::from((ip2, port2.parse().unwrap()));
                    let addrs = vec![addr1, addr2];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let (mut handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!( HTTP ; server_fn );
        let mut handle2 = start_http_server!( HTTP ; server_fn );
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
        .dns_resolver(ExampleDnsResolver)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", handle1.port, handle2.port).as_str())
        .body(Body::empty())
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_ok());

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
fn sdv_client_request_dns_resolver_ipv6_multiple2() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let (port1, port2) = port.split_once(':').unwrap();
                    let ip1 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
                    let addr1 = SocketAddr::from((ip1, port1.parse().unwrap()));
                    let ip2 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
                    let addr2 = SocketAddr::from((ip2, port2.parse().unwrap()));
                    let addrs = vec![addr1, addr2];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let (mut handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!( HTTP ; Ipv6; server_fn );
        let mut handle2 = start_http_server!( HTTP ; Ipv6; server_fn );
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
        .dns_resolver(ExampleDnsResolver)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", handle1.port, handle2.port).as_str())
        .body(Body::empty())
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_ok());

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
fn sdv_client_request_dns_resolver_ipv4_ipv6() {
    struct ExampleDnsResolver;
    impl Resolver for ExampleDnsResolver {
        fn resolve(&self, authority: &str) -> SocketFuture {
            let port = authority.to_string();
            Box::pin(async move {
                if port.is_empty() {
                    Err(io::Error::new(io::ErrorKind::Other, "").into())
                } else {
                    let (port1, port2) = port.split_once(':').unwrap();
                    let ip1 = Ipv4Addr::new(0, 0, 0, 1);
                    let addr1 = SocketAddr::from((ip1, port1.parse().unwrap()));
                    let ip2 = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
                    let addr2 = SocketAddr::from((ip2, port2.parse().unwrap()));
                    let addrs = vec![addr1, addr2];
                    Ok(Box::new(addrs.into_iter())
                        as Box<dyn Iterator<Item = SocketAddr> + Sync + Send>)
                }
            })
        }
    }
    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let (mut handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!( HTTP ; server_fn );
        let mut handle2 = start_http_server!( HTTP ; Ipv6; server_fn );
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
        .dns_resolver(ExampleDnsResolver)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", handle1.port, handle2.port).as_str())
        .body(Body::empty())
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_ok());

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
