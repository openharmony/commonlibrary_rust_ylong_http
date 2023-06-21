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

use crate::util::ConnectorConfig;
use crate::{AsyncRead, AsyncWrite};
use core::future::Future;
use ylong_http::request::uri::Uri;

/// `Connector` trait used by `async_impl::Client`. `Connector` provides
/// asynchronous connection establishment interfaces.
#[rustfmt::skip]
pub trait Connector {
    /// The type of stream that this connector produces. This must be an async stream that implements AsyncRead, AsyncWrite, and is also Send + Sync.
    type Stream: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static;
    /// The type of errors that this connector can produce when trying to create a stream.
    type Error: Into<Box<dyn std::error::Error + Sync + Send>>;
    /// The future type returned by this connector when attempting to create a stream.
    type Future: Future<Output = Result<Self::Stream, Self::Error>> + Unpin + Sync + Send + 'static;

    /// Attempts to establish a connection.
    fn connect(&self, uri: &Uri) -> Self::Future;
}

/// Connector for creating HTTP connections asynchronously.
///
/// `HttpConnector` implements `async_impl::Connector` trait.
pub struct HttpConnector {
    config: ConnectorConfig,
}

impl HttpConnector {
    /// Creates a new `HttpConnector`.
    pub(crate) fn new(config: ConnectorConfig) -> HttpConnector {
        HttpConnector { config }
    }
}

impl Default for HttpConnector {
    fn default() -> Self {
        Self::new(ConnectorConfig::default())
    }
}

#[cfg(not(feature = "__tls"))]
pub(crate) mod no_tls {
    use crate::async_impl::Connector;
    use crate::TcpStream;
    use core::{future::Future, pin::Pin};
    use std::io::Error;
    use ylong_http::request::uri::Uri;

    impl Connector for super::HttpConnector {
        type Stream = TcpStream;
        type Error = Error;
        type Future =
            Pin<Box<dyn Future<Output = Result<Self::Stream, Self::Error>> + Sync + Send>>;

        fn connect(&self, uri: &Uri) -> Self::Future {
            let addr = if let Some(proxy) = self.config.proxies.match_proxy(uri) {
                proxy.via_proxy(uri).authority().unwrap().to_string()
            } else {
                uri.authority().unwrap().to_string()
            };
            Box::pin(async move {
                TcpStream::connect(addr)
                    .await
                    .and_then(|stream| match stream.set_nodelay(true) {
                        Ok(()) => Ok(stream),
                        Err(e) => Err(e),
                    })
            })
        }
    }
}

#[cfg(feature = "__c_openssl")]
pub(crate) mod c_ssl {
    use crate::{
        async_impl::{AsyncSslStream, Connector, MixStream},
        ErrorKind, HttpClientError,
    };
    use crate::{AsyncReadExt, AsyncWriteExt, TcpStream};
    use core::{future::Future, pin::Pin};
    use std::io::Write;
    use ylong_http::request::uri::{Scheme, Uri};

    impl Connector for super::HttpConnector {
        type Stream = MixStream<TcpStream>;
        type Error = HttpClientError;
        type Future =
            Pin<Box<dyn Future<Output = Result<Self::Stream, Self::Error>> + Sync + Send>>;

        fn connect(&self, uri: &Uri) -> Self::Future {
            // Make sure all parts of uri is accurate.
            let mut addr = uri.authority().unwrap().to_string();
            let host = uri.host().unwrap().as_str().to_string();
            let port = uri.port().unwrap().as_u16().unwrap();
            let mut auth = None;
            let mut is_proxy = false;

            if let Some(proxy) = self.config.proxies.match_proxy(uri) {
                addr = proxy.via_proxy(uri).authority().unwrap().to_string();
                auth = proxy
                    .intercept
                    .proxy_info()
                    .basic_auth
                    .as_ref()
                    .and_then(|v| v.to_str().ok());
                is_proxy = true;
            }

            let host_name = match uri.host() {
                Some(host) => host.to_string(),
                None => "no host in uri".to_string(),
            };

            match *uri.scheme().unwrap() {
                Scheme::HTTP => Box::pin(async move {
                    let stream = TcpStream::connect(addr)
                        .await
                        .and_then(|stream| match stream.set_nodelay(true) {
                            Ok(()) => Ok(stream),
                            Err(e) => Err(e),
                        })
                        .map_err(|e| {
                            HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                        })?;
                    Ok(MixStream::Http(stream))
                }),
                Scheme::HTTPS => {
                    let config = self.config.tls.clone();
                    Box::pin(async move {
                        let tcp_stream = TcpStream::connect(addr)
                            .await
                            .and_then(|stream| match stream.set_nodelay(true) {
                                Ok(()) => Ok(stream),
                                Err(e) => Err(e),
                            })
                            .map_err(|e| {
                                HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                            })?;

                        let tcp_stream = if is_proxy {
                            tunnel(tcp_stream, host, port, auth).await?
                        } else {
                            tcp_stream
                        };

                        let mut tls_ssl = config.ssl().map_err(|e| {
                            HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                        })?;

                        tls_ssl.set_sni_verify(&host_name).map_err(|e| {
                            HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                        })?;

                        let mut stream = AsyncSslStream::new(tls_ssl.into_inner(), tcp_stream)
                            .map_err(|e| {
                                HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                            })?;
                        Pin::new(&mut stream).connect().await.map_err(|e| {
                            HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                        })?;
                        Ok(MixStream::Https(stream))
                    })
                }
            }
        }
    }

    async fn tunnel(
        mut conn: TcpStream,
        host: String,
        port: u16,
        auth: Option<String>,
    ) -> Result<TcpStream, HttpClientError> {
        let mut req = Vec::new();

        // `unwrap()` never failed here.
        write!(
            &mut req,
            "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n"
        )
        .unwrap();

        if let Some(value) = auth {
            write!(&mut req, "Proxy-Authorization: Basic {value}\r\n").unwrap();
        }

        write!(&mut req, "\r\n").unwrap();

        conn.write_all(&req)
            .await
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Connect, Some(e)))?;

        let mut buf = [0; 8192];
        let mut pos = 0;

        loop {
            let n = conn
                .read(&mut buf[pos..])
                .await
                .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Connect, Some(e)))?;

            if n == 0 {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Connect,
                    "Error receiving from proxy",
                ));
            }

            pos += n;
            let resp = &buf[..pos];
            if resp.starts_with(b"HTTP/1.1 200") {
                if resp.ends_with(b"\r\n\r\n") {
                    return Ok(conn);
                }
                if pos == buf.len() {
                    return Err(HttpClientError::new_with_message(
                        ErrorKind::Connect,
                        "proxy headers too long for tunnel",
                    ));
                }
            } else if resp.starts_with(b"HTTP/1.1 407") {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Connect,
                    "proxy authentication required",
                ));
            } else {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Connect,
                    "unsuccessful tunnel",
                ));
            }
        }
    }
}
