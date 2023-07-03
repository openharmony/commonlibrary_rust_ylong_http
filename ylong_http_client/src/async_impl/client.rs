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

use super::{conn, Body, ConnPool, Connector, HttpBody, HttpConnector};
use crate::async_impl::timeout::TimeoutFuture;
use crate::util::normalizer::{RequestFormatter, UriFormatter};
use crate::util::proxy::Proxies;
use crate::util::redirect::TriggerKind;
use crate::util::{ClientConfig, ConnectorConfig, HttpConfig, HttpVersion, Redirect};
use crate::{sleep, timeout};
use crate::{ErrorKind, HttpClientError, Proxy, Request, Timeout, Uri};
use ylong_http::body::{ChunkBody, TextBody};
use ylong_http::response::Response;

#[cfg(feature = "http2")]
use crate::H2Config;

/// HTTP asynchronous client implementation. Users can use `async_impl::Client` to
/// send `Request` asynchronously. `async_impl::Client` depends on a
/// [`async_impl::Connector`] that can be customized by the user.
///
/// [`async_impl::Connector`]: Connector
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::Client;
/// use ylong_http_client::{Request, EmptyBody};
///
/// async fn async_client() {
///     // Creates a new `Client`.
///     let client = Client::new();
///
///     // Creates a new `Request`.
///     let request = Request::new(EmptyBody);
///
///     // Sends `Request` and wait for the `Response` to return asynchronously.
///     let response = client.request(request).await.unwrap();
///
///     // Gets the content of `Response`.
///     let status = response.status();
/// }
/// ```
pub struct Client<C: Connector> {
    inner: ConnPool<C, C::Stream>,
    client_config: ClientConfig,
    http_config: HttpConfig,
}

impl Client<HttpConnector> {
    /// Creates a new, default `AsyncClient`, which uses [`async_impl::HttpConnector`].
    ///
    /// [`async_impl::HttpConnector`]: HttpConnector
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Client;
    ///
    /// let client = Client::new();
    /// ```
    pub fn new() -> Self {
        Self::with_connector(HttpConnector::default())
    }

    /// Creates a new, default [`async_impl::ClientBuilder`].
    ///
    /// [`async_impl::ClientBuilder`]: ClientBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Client;
    ///
    /// let builder = Client::builder();
    /// ```
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }
}

impl<C: Connector> Client<C> {
    /// Creates a new, default `AsyncClient` with a given connector.
    pub fn with_connector(connector: C) -> Self {
        let http_config = HttpConfig::default();
        Self {
            inner: ConnPool::new(http_config.clone(), connector),
            client_config: ClientConfig::default(),
            http_config,
        }
    }

    /// Sends HTTP `Request` asynchronously.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Client;
    /// use ylong_http_client::{Request, EmptyBody};
    ///
    /// async fn async_client() {
    ///     let client = Client::new();
    ///     let response = client.request(Request::new(EmptyBody)).await;
    /// }
    /// ```
    // TODO: change result to `Response<HttpBody>` later.
    pub async fn request<T: Body>(
        &self,
        request: Request<T>,
    ) -> Result<super::Response, HttpClientError> {
        let (part, body) = request.into_parts();

        let content_length = part
            .headers
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .is_some();

        let transfer_encoding = part
            .headers
            .get("Transfer-Encoding")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("chunked"))
            .unwrap_or(false);

        let response = match (content_length, transfer_encoding) {
            (_, true) => {
                let request = Request::from_raw_parts(part, ChunkBody::from_async_body(body));
                self.retry_send_request(request).await
            }
            (true, false) => {
                let request = Request::from_raw_parts(part, TextBody::from_async_body(body));
                self.retry_send_request(request).await
            }
            (false, false) => {
                let request = Request::from_raw_parts(part, body);
                self.retry_send_request(request).await
            }
        };
        response.map(super::Response::new)
    }

    async fn retry_send_request<T: Body>(
        &self,
        mut request: Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let mut retries = self.client_config.retry.times().unwrap_or(0);
        loop {
            let response = self.send_request_retryable(&mut request).await;
            if response.is_ok() || retries == 0 {
                return response;
            }
            retries -= 1;
        }
    }

    async fn send_request_retryable<T: Body>(
        &self,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let response = self
            .send_request_with_uri(request.uri().clone(), request)
            .await?;
        self.redirect_request(response, request).await
    }

    async fn redirect_request<T: Body>(
        &self,
        mut response: Response<HttpBody>,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let mut redirected_list = vec![];
        let mut dst_uri = Uri::default();
        loop {
            if Redirect::is_redirect(response.status().clone(), request) {
                redirected_list.push(request.uri().clone());
                let trigger = Redirect::get_redirect(
                    &mut dst_uri,
                    &self.client_config.redirect,
                    &redirected_list,
                    &response,
                    request,
                )?;

                UriFormatter::new().format(&mut dst_uri)?;
                let _ = request
                    .headers_mut()
                    .insert("Host", dst_uri.authority().unwrap().to_string().as_bytes());
                match trigger {
                    TriggerKind::NextLink => {
                        response = self.send_request_with_uri(dst_uri.clone(), request).await?;
                        continue;
                    }
                    TriggerKind::Stop => {
                        return Ok(response);
                    }
                }
            } else {
                return Ok(response);
            }
        }
    }

    async fn send_request_with_uri<T: Body>(
        &self,
        mut uri: Uri,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        UriFormatter::new().format(&mut uri)?;
        RequestFormatter::new(request).normalize()?;

        match self.http_config.version {
            #[cfg(feature = "http2")]
            HttpVersion::Http2PriorKnowledge => self.http2_request(uri, request).await,
            HttpVersion::Http11 => {
                let conn = if let Some(dur) = self.client_config.connect_timeout.inner() {
                    match timeout(dur, self.inner.connect_to(uri)).await {
                        Err(_elapsed) => {
                            return Err(HttpClientError::new_with_message(
                                ErrorKind::Timeout,
                                "Connect timeout",
                            ))
                        }
                        Ok(Ok(conn)) => conn,
                        Ok(Err(e)) => return Err(e),
                    }
                } else {
                    self.inner.connect_to(uri).await?
                };

                let mut retryable = Retryable::default();
                if let Some(timeout) = self.client_config.request_timeout.inner() {
                    TimeoutFuture {
                        timeout: Some(Box::pin(sleep(timeout))),
                        future: Box::pin(conn::request(conn, request, &mut retryable)),
                    }
                    .await
                } else {
                    conn::request(conn, request, &mut retryable).await
                }
            }
        }
    }

    #[cfg(feature = "http2")]
    async fn http2_request<T: Body>(
        &self,
        uri: Uri,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let mut retryable = Retryable::default();

        const RETRY: usize = 1;
        let mut times = 0;
        loop {
            retryable.set_retry(false);
            let conn = self.inner.connect_to(uri.clone()).await?;
            let response = conn::request(conn, request, &mut retryable).await;
            if retryable.retry() && times < RETRY {
                times += 1;
                continue;
            }
            return response;
        }
    }
}

impl Default for Client<HttpConnector> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
pub(crate) struct Retryable {
    #[cfg(feature = "http2")]
    retry: bool,
}

#[cfg(feature = "http2")]
impl Retryable {
    pub(crate) fn set_retry(&mut self, retryable: bool) {
        self.retry = retryable
    }

    pub(crate) fn retry(&self) -> bool {
        self.retry
    }
}

/// A builder which is used to construct `async_impl::Client`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::ClientBuilder;
///
/// let client = ClientBuilder::new().build();
/// ```
pub struct ClientBuilder {
    /// Options and flags that is related to `HTTP`.
    http: HttpConfig,

    /// Options and flags that is related to `Client`.
    client: ClientConfig,

    /// Options and flags that is related to `Proxy`.
    proxies: Proxies,

    /// Options and flags that is related to `TLS`.
    #[cfg(feature = "__tls")]
    tls: crate::util::TlsConfigBuilder,
}

impl ClientBuilder {
    /// Creates a new, default `ClientBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            http: HttpConfig::default(),
            client: ClientConfig::default(),
            proxies: Proxies::default(),

            #[cfg(feature = "__tls")]
            tls: crate::util::TlsConfig::builder(),
        }
    }

    /// Only use HTTP/1.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().http1_only();
    /// ```
    pub fn http1_only(mut self) -> Self {
        self.http.version = HttpVersion::Http11;
        self
    }

    /// Only use HTTP/2.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().http2_prior_knowledge();
    /// ```
    #[cfg(feature = "http2")]
    pub fn http2_prior_knowledge(mut self) -> Self {
        self.http.version = HttpVersion::Http2PriorKnowledge;
        self
    }

    /// HTTP/2 settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::H2Config;
    ///
    /// let builder = ClientBuilder::new().http2_settings(H2Config::default());
    /// ```
    #[cfg(feature = "http2")]
    pub fn http2_settings(mut self, config: H2Config) -> Self {
        self.http.http2_config = config;
        self
    }

    /// Enables a request timeout.
    ///
    /// The timeout is applied from when the request starts connection util the
    /// response body has finished.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::Timeout;
    ///
    /// let builder = ClientBuilder::new().request_timeout(Timeout::none());
    /// ```
    pub fn request_timeout(mut self, timeout: Timeout) -> Self {
        self.client.request_timeout = timeout;
        self
    }

    /// Sets a timeout for only the connect phase of `Client`.
    ///
    /// Default is `Timeout::none()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::Timeout;
    ///
    /// let builder = ClientBuilder::new().connect_timeout(Timeout::none());
    /// ```
    pub fn connect_timeout(mut self, timeout: Timeout) -> Self {
        self.client.connect_timeout = timeout;
        self
    }

    /// Sets a `Redirect` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::Redirect;
    ///
    /// let builder = ClientBuilder::new().redirect(Redirect::none());
    /// ```
    pub fn redirect(mut self, redirect: Redirect) -> Self {
        self.client.redirect = redirect;
        self
    }

    /// Adds a `Proxy` to the list of proxies the `Client` will use.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::ClientBuilder;
    /// # use ylong_http_client::{HttpClientError, Proxy};
    ///
    /// # fn add_proxy() -> Result<(), HttpClientError> {
    /// let builder = ClientBuilder::new().proxy(Proxy::http("http://www.example.com").build()?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn proxy(mut self, proxy: Proxy) -> Self {
        self.proxies.add_proxy(proxy.inner());
        self
    }

    /// Sets the maximum allowed TLS version for connections.
    ///
    /// By default there's no maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::TlsVersion;
    ///
    /// let builder = ClientBuilder::new().max_tls_version(TlsVersion::TLS_1_2);
    /// ```
    #[cfg(feature = "__tls")]
    pub fn max_tls_version(mut self, version: crate::util::TlsVersion) -> Self {
        self.tls = self.tls.set_max_proto_version(version);
        self
    }

    /// Sets the minimum required TLS version for connections.
    ///
    /// By default the TLS backend's own default is used.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::TlsVersion;
    ///
    /// let builder = ClientBuilder::new().min_tls_version(TlsVersion::TLS_1_2);
    /// ```
    #[cfg(feature = "__tls")]
    pub fn min_tls_version(mut self, version: crate::util::TlsVersion) -> Self {
        self.tls = self.tls.set_min_proto_version(version);
        self
    }

    /// Adds a custom root certificate.
    ///
    /// This can be used to connect to a server that has a self-signed.
    /// certificate for example.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::Certificate;
    ///
    /// # fn set_cert(cert: Certificate) {
    /// let builder = ClientBuilder::new().add_root_certificate(cert);
    /// # }
    /// ```
    #[cfg(feature = "__tls")]
    pub fn add_root_certificate(mut self, certs: crate::util::Certificate) -> Self {
        self.tls = self.tls.add_root_certificates(certs);
        self
    }

    /// Loads trusted root certificates from a file. The file should contain a
    /// sequence of PEM-formatted CA certificates.
    #[cfg(feature = "__tls")]
    pub fn set_ca_file(mut self, path: &str) -> Self {
        self.tls = self.tls.set_ca_file(path);
        self
    }

    /// Sets the list of supported ciphers for protocols before `TLSv1.3`.
    ///
    /// See [`ciphers`] for details on the format.
    ///
    /// [`ciphers`]: https://www.openssl.org/docs/man1.1.0/apps/ciphers.html
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new()
    ///     .set_cipher_list(
    ///         "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK"
    ///     );
    /// ```
    #[cfg(feature = "__tls")]
    pub fn set_cipher_list(mut self, list: &str) -> Self {
        self.tls = self.tls.set_cipher_list(list);
        self
    }

    /// Sets the list of supported ciphers for the `TLSv1.3` protocol.
    ///
    /// The format consists of TLSv1.3 cipher suite names separated by `:`
    /// characters in order of preference.
    ///
    /// Requires `OpenSSL 1.1.1` or `LibreSSL 3.4.0` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new()
    ///     .set_cipher_suites(
    ///         "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK"
    ///     );
    /// ```
    #[cfg(feature = "__tls")]
    pub fn set_cipher_suites(mut self, list: &str) -> Self {
        self.tls = self.tls.set_cipher_suites(list);
        self
    }

    /// Controls the use of built-in system certificates during certificate validation.
    /// Default to `true` -- uses built-in system certs.
    #[cfg(feature = "__tls")]
    pub fn tls_built_in_root_certs(mut self, is_use: bool) -> ClientBuilder {
        self.tls = self.tls.build_in_root_certs(is_use);
        self
    }

    /// Constructs a `Client` based on the given settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let client = ClientBuilder::new().build();
    /// ```
    pub fn build(self) -> Result<Client<HttpConnector>, HttpClientError> {
        let config = ConnectorConfig {
            proxies: self.proxies,
            #[cfg(feature = "__tls")]
            tls: self.tls.build()?,
        };

        let connector = HttpConnector::new(config);

        Ok(Client {
            inner: ConnPool::new(self.http.clone(), connector),
            client_config: self.client,
            http_config: self.http,
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod ut_async_impl_client {
    use crate::async_impl::Client;

    /// UT test cases for `Client::builder`.
    ///
    /// # Brief
    /// 1. Creates a ClientBuilder by calling `Client::Builder`.
    /// 2. Calls `http_config`, `client_config`, `build` on the builder respectively.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_client_builder() {
        let builder = Client::builder().http1_only().build();
        assert!(builder.is_ok())
    }

    /// UT test cases for `ClientBuilder::default`.
    ///
    /// # Brief
    /// 1. Creates a `ClientBuilder` by calling `ClientBuilder::default`.
    /// 2. Calls `http_config`, `client_config`, `tls_config` and `build` respectively.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "__tls")]
    #[test]
    fn ut_client_builder_default() {
        use crate::async_impl::ClientBuilder;
        use crate::util::{Redirect, Timeout};

        let builder = ClientBuilder::default()
            .redirect(Redirect::none())
            .connect_timeout(Timeout::from_secs(9))
            .build();
        assert!(builder.is_ok())
    }
}
