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

#![cfg(all(feature = "async", feature = "tokio_base", not(feature = "__tls")))]

mod common;

use std::convert::Infallible;

use ylong_http::response::status::StatusCode;
use ylong_http_client::async_impl::{Body, Client, HttpBody, Request, Response};
use ylong_http_client::{ConnDetail, ErrorKind, HttpClientError, Interceptor};

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

async fn server_fn_redirect(
    req: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    use hyper::body::HttpBody;
    let mut body = req.into_body();

    let mut buf = vec![];
    loop {
        match body.data().await {
            None => {
                break;
            }
            Some(Ok(bytes)) => buf.extend_from_slice(bytes.as_ref()),
            Some(Err(_e)) => {
                panic!("server read request body data occurs error");
            }
        }
    }
    let redirect_addr = format!("127.0.0.1:{}", std::str::from_utf8(&buf).unwrap());
    let response = hyper::Response::builder()
        .header("Location", redirect_addr)
        .status(hyper::StatusCode::TEMPORARY_REDIRECT)
        .body(hyper::Body::empty())
        .expect("build hyper response failed");
    Ok(response)
}

macro_rules! interceptor_test {
    (
        $interceptor: ident,
        $service_fn: ident,
        $version: literal,
        Success;
    ) => {
        define_service_handle!(HTTP;);

        let rt = init_test_work_runtime(4);

        let mut handle = rt.block_on(async move {
            let mut handle = start_http_server!(HTTP; $service_fn);
            handle.server_start.recv().await.unwrap();
            handle
        });

        let client = Client::builder();
        let client = match $version {
            #[cfg(feature = "http1_1")]
            "HTTP/1.1" => client.http1_only(),
            #[cfg(feature = "http2")]
            "HTTP/2.0" => client.http2_prior_knowledge(),
            _ => client
        };
        let client = client
            .interceptor($interceptor)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
            .version($version)
            .method("GET")
            .body(Body::empty())
            .expect("Client build Request failed.");

        rt.block_on(async move {
            let response = client.request(request).await.expect("get response failed");
            assert_eq!(response.status(), StatusCode::OK);

            handle.client_shutdown.send(()).await.unwrap();
            handle.server_shutdown.recv().await.unwrap();
        })
    };
    (
        $interceptor: ident,
        $service_fn: ident,
        $version: literal,
        Fail;
    ) => {
        define_service_handle!(HTTP;);

        let rt = init_test_work_runtime(4);

        let handle = rt.block_on(async move {
            let mut handle = start_http_server!(HTTP; $service_fn);
            handle.server_start.recv().await.unwrap();
            handle
        });

        let client = Client::builder();
        let client = match $version {
            #[cfg(feature = "http1_1")]
            "HTTP/1.1" => client.http1_only(),
            #[cfg(feature = "http2")]
            "HTTP/2.0" => client.http2_prior_knowledge(),
            _ => client
        };
        let client = client
            .interceptor($interceptor)
            .build()
            .expect("Build Client failed.");

        let request = Request::builder()
            .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
            .version($version)
            .method("GET")
            .body(Body::empty())
            .expect("Client build Request failed.");

        rt.block_on(async move {
            let response = client.request(request).await;
            assert!(response.is_err());
            assert_eq!(response.err().unwrap().error_kind(), ErrorKind::UserAborted);

            handle.client_shutdown.send(()).await.unwrap();
        })
    };
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_ok() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {}

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Success;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_connection() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_connection(&self, _info: ConnDetail) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_input() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_input(&self, _bytes: &[u8]) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_output() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_output(&self, _bytes: &[u8]) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_request() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_request(&self, _request: &Request) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_response() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_response(&self, _response: &Response) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_retry() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_connection(&self, _info: ConnDetail) -> Result<(), HttpClientError> {
            Err(HttpClientError::other("other"))
        }
        fn intercept_retry(&self, _error: &HttpClientError) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let handle = rt.block_on(async move {
        let mut handle = start_http_server!( HTTP ; server_fn );
        handle.server_start.recv().await.unwrap();
        handle
    });
    let client = Client::builder()
        .retry(ylong_http_client::Retry::new(2).unwrap())
        .interceptor(ExampleInterceptor)
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
        assert_eq!(response.err().unwrap().error_kind(), ErrorKind::UserAborted);

        handle.client_shutdown.send(()).await.unwrap();
    })
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_redirect_request() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_redirect_request(&self, _request: &Request) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn_redirect, "HTTP/1.1", Fail;);
}

#[test]
#[cfg(feature = "http1_1")]
fn sdv_client_request_interceptor_http1_redirect_response() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_redirect_response(
            &self,
            _response: &ylong_http::response::Response<HttpBody>,
        ) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let (handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!( HTTP ; server_fn_redirect );
        let mut handle2 = start_http_server!( HTTP ; server_fn );
        handle1.server_start.recv().await.unwrap();
        handle2.server_start.recv().await.unwrap();
        (handle1, handle2)
    });
    let client = Client::builder()
        .interceptor(ExampleInterceptor)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle1.port).as_str())
        .version("HTTP/1.1")
        .method("GET")
        .body(Body::slice(handle2.port.to_string().as_str()))
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());
        assert_eq!(response.err().unwrap().error_kind(), ErrorKind::UserAborted);

        handle1.client_shutdown.send(()).await.unwrap();
        handle2.client_shutdown.send(()).await.unwrap();
        handle2.server_shutdown.recv().await.unwrap();
    })
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_ok() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {}

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/2.0", Success;);
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_connection() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_connection(&self, _info: ConnDetail) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/2.0", Fail;);
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_request() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_request(&self, _request: &Request) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/2.0", Fail;);
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_response() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_response(&self, _response: &Response) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn, "HTTP/2.0", Fail;);
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_retry() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_connection(&self, _info: ConnDetail) -> Result<(), HttpClientError> {
            Err(HttpClientError::other("other"))
        }
        fn intercept_retry(&self, _error: &HttpClientError) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let handle = rt.block_on(async move {
        let mut handle = start_http_server!( HTTP ; server_fn );
        handle.server_start.recv().await.unwrap();
        handle
    });
    let client = Client::builder()
        .http2_prior_knowledge()
        .retry(ylong_http_client::Retry::new(2).unwrap())
        .interceptor(ExampleInterceptor)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle.port).as_str())
        .version("HTTP/2.0")
        .method("GET")
        .body(Body::empty())
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());
        assert_eq!(response.err().unwrap().error_kind(), ErrorKind::UserAborted);

        handle.client_shutdown.send(()).await.unwrap();
    })
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_redirect_request() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_redirect_request(&self, _request: &Request) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    interceptor_test!(ExampleInterceptor, server_fn_redirect, "HTTP/2.0", Fail;);
}

#[test]
#[cfg(feature = "http2")]
fn sdv_client_request_interceptor_http2_redirect_response() {
    struct ExampleInterceptor;
    impl Interceptor for ExampleInterceptor {
        fn intercept_redirect_response(
            &self,
            _response: &ylong_http::response::Response<HttpBody>,
        ) -> Result<(), HttpClientError> {
            Err(HttpClientError::user_aborted())
        }
    }

    define_service_handle!( HTTP ; );
    let rt = init_test_work_runtime(4);
    let (handle1, mut handle2) = rt.block_on(async move {
        let mut handle1 = start_http_server!( HTTP ; server_fn_redirect );
        let mut handle2 = start_http_server!( HTTP ; server_fn );
        handle1.server_start.recv().await.unwrap();
        handle2.server_start.recv().await.unwrap();
        (handle1, handle2)
    });
    let client = Client::builder()
        .http2_prior_knowledge()
        .interceptor(ExampleInterceptor)
        .build()
        .expect("Build Client failed.");
    let request = Request::builder()
        .url(format!("{}:{}", "127.0.0.1", handle1.port).as_str())
        .version("HTTP/2.0")
        .method("GET")
        .body(Body::slice(handle2.port.to_string().as_str()))
        .expect("Client build Request failed.");
    rt.block_on(async move {
        let response = client.request(request).await;
        assert!(response.is_err());
        assert_eq!(response.err().unwrap().error_kind(), ErrorKind::UserAborted);

        handle1.client_shutdown.send(()).await.unwrap();
        handle2.client_shutdown.send(()).await.unwrap();
        handle2.server_shutdown.recv().await.unwrap();
    })
}
