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

use std::io::{Read, Write};

use ylong_http::request::uri::Uri;

use crate::util::config::ConnectorConfig;

/// `Connector` trait used by `Client`. `Connector` provides synchronous
/// connection establishment interfaces.
pub trait Connector {
    /// The connection object established by `Connector::connect`.
    type Stream: Read + Write + 'static;
    /// Possible errors during connection establishment.
    type Error: Into<Box<dyn std::error::Error + Send + Sync>>;

    /// Attempts to establish a synchronous connection.
    fn connect(&self, uri: &Uri) -> Result<Self::Stream, Self::Error>;
}

/// Connector for creating HTTP connections synchronously.
///
/// `HttpConnector` implements `sync_impl::Connector` trait.
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
pub mod no_tls {
    use std::io::Error;
    use std::net::TcpStream;

    use ylong_http::request::uri::Uri;

    use crate::sync_impl::Connector;

    impl Connector for super::HttpConnector {
        type Stream = TcpStream;
        type Error = Error;

        fn connect(&self, uri: &Uri) -> Result<Self::Stream, Self::Error> {
            let addr = if let Some(proxy) = self.config.proxies.match_proxy(uri) {
                proxy.via_proxy(uri).authority().unwrap().to_string()
            } else {
                uri.authority().unwrap().to_string()
            };
            TcpStream::connect(addr)
        }
    }
}

#[cfg(feature = "__tls")]
pub mod tls_conn {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use ylong_http::request::uri::{Scheme, Uri};

    use crate::sync_impl::{Connector, MixStream};
    use crate::{ErrorKind, HttpClientError};

    impl Connector for super::HttpConnector {
        type Stream = MixStream<TcpStream>;
        type Error = HttpClientError;

        fn connect(&self, uri: &Uri) -> Result<Self::Stream, Self::Error> {
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
                    .and_then(|v| v.to_string().ok());
                is_proxy = true;
            }

            let host_name = match uri.host() {
                Some(host) => host.to_string(),
                None => "no host in uri".to_string(),
            };

            match *uri.scheme().unwrap() {
                Scheme::HTTP => {
                    Ok(MixStream::Http(TcpStream::connect(addr).map_err(|e| {
                        HttpClientError::from_error(ErrorKind::Connect, e)
                    })?))
                }
                Scheme::HTTPS => {
                    let tcp_stream = TcpStream::connect(addr)
                        .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;

                    let tcp_stream = if is_proxy {
                        tunnel(tcp_stream, host, port, auth)?
                    } else {
                        tcp_stream
                    };

                    let tls_ssl = self
                        .config
                        .tls
                        .ssl_new(&host_name)
                        .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;

                    let stream = tls_ssl
                        .into_inner()
                        .connect(tcp_stream)
                        .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
                    Ok(MixStream::Https(stream))
                }
            }
        }
    }

    fn tunnel(
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
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;

        let mut buf = [0; 8192];
        let mut pos = 0;

        loop {
            let n = conn
                .read(&mut buf[pos..])
                .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;

            if n == 0 {
                return Err(HttpClientError::from_str(
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
                    return Err(HttpClientError::from_str(
                        ErrorKind::Connect,
                        "proxy headers too long for tunnel",
                    ));
                }
            } else if resp.starts_with(b"HTTP/1.1 407") {
                return Err(HttpClientError::from_str(
                    ErrorKind::Connect,
                    "proxy authentication required",
                ));
            } else {
                return Err(HttpClientError::from_str(
                    ErrorKind::Connect,
                    "unsuccessful tunnel",
                ));
            }
        }
    }
}
