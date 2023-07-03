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

use super::{Body, Connector, HttpBody, HttpConnector};
use crate::error::HttpClientError;
use crate::sync_impl::conn;
use crate::sync_impl::pool::ConnPool;
use crate::util::normalizer::RequestFormatter;
use crate::util::proxy::Proxies;
use crate::util::redirect::TriggerKind;
use crate::util::{ClientConfig, HttpConfig, HttpVersion, Proxy, Timeout};
use crate::util::{ConnectorConfig, Redirect};
use crate::Request;
use ylong_http::request::uri::Uri;

// TODO: Adapter, remove this later.
use ylong_http::response::Response;

/// HTTP synchronous client implementation. Users can use `Client` to
/// send `Request` synchronously. `Client` depends on a `Connector` that
/// can be customized by the user.
///
/// # Examples
///
/// ```no_run
/// use ylong_http_client::sync_impl::Client;
/// use ylong_http_client::{Request, Response, EmptyBody};
///
/// // Creates a new `Client`.
/// let client = Client::new();
///
/// // Creates a new `Request`.
/// let request = Request::new(EmptyBody);
///
/// // Sends `Request` and block waiting for `Response` to return.
/// let response = client.request(request).unwrap();
///
/// // Gets the content of `Response`.
/// let status = response.status();
/// ```
pub struct Client<C: Connector> {
    inner: ConnPool<C, C::Stream>,
    client_config: ClientConfig,
}

impl Client<HttpConnector> {
    /// Creates a new, default `Client`, which uses [`sync_impl::HttpConnector`].
    ///
    /// [`sync_impl::HttpConnector`]: HttpConnector
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::Client;
    ///
    /// let client = Client::new();
    /// ```
    pub fn new() -> Self {
        Self::with_connector(HttpConnector::default())
    }

    /// Creates a new, default [`sync_impl::ClientBuilder`].
    ///
    /// [`sync_impl::ClientBuilder`]: ClientBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::Client;
    ///
    /// let builder = Client::builder();
    /// ```
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }
}

impl<C: Connector> Client<C> {
    /// Creates a new, default `Client` with a given connector.
    pub fn with_connector(connector: C) -> Self {
        Self {
            inner: ConnPool::new(connector),
            client_config: ClientConfig::new(),
        }
    }

    /// Sends HTTP Request synchronously. This method will block the current
    /// thread until a `Response` is obtained or an error occurs.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_http_client::sync_impl::Client;
    /// use ylong_http_client::{Request, EmptyBody};
    ///
    /// let client = Client::new();
    /// let response = client.request(Request::new(EmptyBody));
    /// ```
    pub fn request<T: Body>(
        &self,
        mut request: Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        RequestFormatter::new(&mut request).normalize()?;
        self.retry_send_request(request)
    }

    fn retry_send_request<T: Body>(
        &self,
        mut request: Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let mut retries = self.client_config.retry.times().unwrap_or(0);
        loop {
            let response = self.send_request_retryable(&mut request);
            if response.is_ok() || retries == 0 {
                return response;
            }
            retries -= 1;
        }
    }

    fn send_request_retryable<T: Body>(
        &self,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        let response = self.send_request_with_uri(request.uri().clone(), request)?;
        self.redirect_request(response, request)
    }

    fn redirect_request<T: Body>(
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

                match trigger {
                    TriggerKind::NextLink => {
                        response = conn::request(self.inner.connect_to(dst_uri.clone())?, request)?;
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

    fn send_request_with_uri<T: Body>(
        &self,
        uri: Uri,
        request: &mut Request<T>,
    ) -> Result<Response<HttpBody>, HttpClientError> {
        conn::request(self.inner.connect_to(uri)?, request)
    }
}

impl Default for Client<HttpConnector> {
    fn default() -> Self {
        Self::new()
    }
}

/// A builder which is used to construct `sync_impl::Client`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().http1_only();
    /// ```
    pub fn http1_only(mut self) -> Self {
        self.http.version = HttpVersion::Http11;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// # use ylong_http_client::sync_impl::ClientBuilder;
    /// # use ylong_http_client::{HttpClientError, Proxy};
    ///
    /// # fn add_proxy() -> Result<(), HttpClientError> {
    /// let builder = ClientBuilder::new().proxy(Proxy::http("http://www.example.com").build()?);
    /// # Ok(())
    /// # }
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().set_ca_file("ca.crt");
    /// ```
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
    /// use ylong_http_client::sync_impl::ClientBuilder;
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

    /// Constructs a `Client` based on the given settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::ClientBuilder;
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
            inner: ConnPool::new(connector),
            client_config: self.client,
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod ut_syn_client {
    use crate::sync_impl::Client;
    use ylong_http::body::TextBody;
    use ylong_http::request::uri::Uri;
    use ylong_http::request::Request;

    /// UT test cases for `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::new`.
    /// 2. Calls `request`.
    /// 3. Checks if the result is error.
    #[test]
    fn ut_request_client_err() {
        let client = Client::new();
        let reader = "Hello World";
        let body = TextBody::from_bytes(reader.as_bytes());
        let mut req = Request::new(body);
        let request_uri = req.uri_mut();
        *request_uri = Uri::from_bytes(b"http://_:80").unwrap();
        let response = client.request(req);
        assert!(response.is_err())
    }

    /// UT test cases for `Client::new`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::new`.
    /// 2. Calls `request`.
    /// 3. Checks if the result is correct.
    #[test]
    fn ut_client_new() {
        let _ = Client::default();
        let _ = Client::new();
    }

    /// UT test cases for `Client::builder`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder`.
    /// 2. Calls `http_config`, `client_config`, `tls_config` and `build` respectively.
    /// 3. Checks if the result is correct.
    #[cfg(feature = "__tls")]
    #[test]
    fn ut_client_builder() {
        let builder = Client::builder().build();
        assert!(builder.is_ok());
    }
}
