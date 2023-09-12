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

use ylong_http::request::uri::Uri;

use super::pool::ConnPool;
use super::timeout::TimeoutFuture;
use super::{conn, Body, Connector, HttpConnector, Request, Response};
use crate::error::HttpClientError;
use crate::runtime::timeout;
use crate::util::config::{
    ClientConfig, ConnectorConfig, HttpConfig, HttpVersion, Proxy, Redirect, Timeout,
};
use crate::util::dispatcher::Conn;
use crate::util::normalizer::RequestFormatter;
use crate::util::proxy::Proxies;
use crate::util::redirect::{RedirectInfo, Trigger};
#[cfg(feature = "__tls")]
use crate::CertVerifier;

/// HTTP asynchronous client implementation. Users can use `async_impl::Client`
/// to send `Request` asynchronously.
///
/// `async_impl::Client` depends on a [`async_impl::Connector`] that can be
/// customized by the user.
///
/// [`async_impl::Connector`]: Connector
///
/// # Examples
///
/// ```no_run
/// use ylong_http_client::async_impl::{Body, Client, Request};
/// use ylong_http_client::HttpClientError;
///
/// async fn async_client() -> Result<(), HttpClientError> {
///     // Creates a new `Client`.
///     let client = Client::new();
///
///     // Creates a new `Request`.
///     let request = Request::builder().body(Body::empty())?;
///
///     // Sends `Request` and wait for the `Response` to return asynchronously.
///     let response = client.request(request).await?;
///
///     // Gets the content of `Response`.
///     let status = response.status();
///
///     Ok(())
/// }
/// ```
pub struct Client<C: Connector> {
    inner: ConnPool<C, C::Stream>,
    config: ClientConfig,
}

impl Client<HttpConnector> {
    /// Creates a new, default `Client`, which uses
    /// [`async_impl::HttpConnector`].
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
    /// Creates a new, default `Client` with a given connector.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::{Client, HttpConnector};
    ///
    /// let client = Client::with_connector(HttpConnector::default());
    /// ```
    pub fn with_connector(connector: C) -> Self {
        Self {
            inner: ConnPool::new(HttpConfig::default(), connector),
            config: ClientConfig::default(),
        }
    }

    /// Sends HTTP `Request` asynchronously.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::{Body, Client, Request};
    /// use ylong_http_client::HttpClientError;
    ///
    /// async fn async_client() -> Result<(), HttpClientError> {
    ///     let client = Client::new();
    ///     let response = client
    ///         .request(Request::builder().body(Body::empty())?)
    ///         .await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn request(&self, request: Request) -> Result<Response, HttpClientError> {
        let mut request = request;
        let mut retries = self.config.retry.times().unwrap_or(0);
        loop {
            let response = self.send_request(&mut request).await;
            // Only bodies which are reusable can be retried.
            if response.is_ok() || retries == 0 || !request.body_mut().reuse() {
                return response;
            }
            retries -= 1;
        }
    }
}

impl<C: Connector> Client<C> {
    async fn send_request(&self, request: &mut Request) -> Result<Response, HttpClientError> {
        let response = self.send_unformatted_request(request).await?;
        self.redirect(response, request).await
    }

    async fn send_unformatted_request(
        &self,
        request: &mut Request,
    ) -> Result<Response, HttpClientError> {
        RequestFormatter::new(&mut *request).format()?;
        let conn = self.connect_to(request.uri()).await?;
        self.send_request_on_conn(conn, request).await
    }

    async fn connect_to(&self, uri: &Uri) -> Result<Conn<C::Stream>, HttpClientError> {
        if let Some(dur) = self.config.connect_timeout.inner() {
            match timeout(dur, self.inner.connect_to(uri)).await {
                Err(elapsed) => err_from_other!(Timeout, elapsed),
                Ok(Ok(conn)) => Ok(conn),
                Ok(Err(e)) => Err(e),
            }
        } else {
            self.inner.connect_to(uri).await
        }
    }

    async fn send_request_on_conn(
        &self,
        conn: Conn<C::Stream>,
        request: &mut Request,
    ) -> Result<Response, HttpClientError> {
        if let Some(timeout) = self.config.request_timeout.inner() {
            TimeoutFuture::new(conn::request(conn, request), timeout).await
        } else {
            conn::request(conn, request).await
        }
    }

    async fn redirect(
        &self,
        response: Response,
        request: &mut Request,
    ) -> Result<Response, HttpClientError> {
        let mut response = response;
        let mut info = RedirectInfo::new();
        loop {
            match self
                .config
                .redirect
                .inner()
                .redirect(request, &response, &mut info)?
            {
                Trigger::NextLink => {
                    // Here the body should be reused.
                    if !request.body_mut().reuse() {
                        *request.body_mut() = Body::empty();
                    }
                    response = self.send_unformatted_request(request).await?;
                }
                Trigger::Stop => return Ok(response),
            }
        }
    }
}

impl Default for Client<HttpConnector> {
    fn default() -> Self {
        Self::new()
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

    /// Only use HTTP/1.x.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().http1_only();
    /// ```
    #[cfg(feature = "http1_1")]
    pub fn http1_only(mut self) -> Self {
        self.http.version = HttpVersion::Http1;
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
            inner: ConnPool::new(self.http, connector),
            config: self.client,
        })
    }
}

#[cfg(feature = "http2")]
impl ClientBuilder {
    /// Only use HTTP/2.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().http2_prior_knowledge();
    /// ```
    pub fn http2_prior_knowledge(mut self) -> Self {
        self.http.version = HttpVersion::Http2PriorKnowledge;
        self
    }

    /// Sets the `SETTINGS_MAX_FRAME_SIZE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let config = ClientBuilder::new().set_http2_max_frame_size(2 << 13);
    /// ```
    pub fn set_http2_max_frame_size(mut self, size: u32) -> Self {
        self.http.http2_config.max_frame_size = size;
        self
    }

    /// Sets the `SETTINGS_MAX_HEADER_LIST_SIZE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let config = ClientBuilder::new().set_http2_max_header_list_size(16 << 20);
    /// ```
    pub fn set_http2_max_header_list_size(mut self, size: u32) -> Self {
        self.http.http2_config.max_header_list_size = size;
        self
    }

    /// Sets the `SETTINGS_HEADER_TABLE_SIZE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let config = ClientBuilder::new().set_http2_max_header_list_size(4096);
    /// ```
    pub fn set_http2_header_table_size(mut self, size: u32) -> Self {
        self.http.http2_config.header_table_size = size;
        self
    }
}

#[cfg(feature = "__tls")]
impl ClientBuilder {
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
    pub fn max_tls_version(mut self, version: crate::util::TlsVersion) -> Self {
        self.tls = self.tls.max_proto_version(version);
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
    pub fn min_tls_version(mut self, version: crate::util::TlsVersion) -> Self {
        self.tls = self.tls.min_proto_version(version);
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
    pub fn add_root_certificate(mut self, certs: crate::util::Certificate) -> Self {
        use crate::c_openssl::adapter::CertificateList;

        match certs.into_inner() {
            CertificateList::CertList(c) => {
                self.tls = self.tls.add_root_certificates(c);
            }
            #[cfg(feature = "c_openssl_3_0")]
            CertificateList::PathList(p) => {
                self.tls = self.tls.add_path_certificates(p);
            }
        }
        self
    }

    /// Loads trusted root certificates from a file. The file should contain a
    /// sequence of PEM-formatted CA certificates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().tls_ca_file("ca.crt");
    /// ```
    pub fn tls_ca_file(mut self, path: &str) -> Self {
        self.tls = self.tls.ca_file(path);
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
    ///     .tls_cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK");
    /// ```
    pub fn tls_cipher_list(mut self, list: &str) -> Self {
        self.tls = self.tls.cipher_list(list);
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
    /// let builder = ClientBuilder::new().tls_cipher_suites(
    ///     "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK",
    /// );
    /// ```
    pub fn tls_cipher_suites(mut self, list: &str) -> Self {
        self.tls = self.tls.cipher_suites(list);
        self
    }

    /// Controls the use of built-in system certificates during certificate
    /// validation. Default to `true` -- uses built-in system certs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().tls_built_in_root_certs(false);
    /// ```
    pub fn tls_built_in_root_certs(mut self, is_use: bool) -> Self {
        self.tls = self.tls.build_in_root_certs(is_use);
        self
    }

    /// Controls the use of certificates verification.
    ///
    /// Defaults to `false` -- verify certificates.
    ///
    /// # Warning
    ///
    /// When sets `true`, any certificate for any site will be trusted for use.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().danger_accept_invalid_certs(true);
    /// ```
    pub fn danger_accept_invalid_certs(mut self, is_invalid: bool) -> Self {
        self.tls = self.tls.danger_accept_invalid_certs(is_invalid);
        self
    }

    /// Controls the use of hostname verification.
    ///
    /// Defaults to `false` -- verify hostname.
    ///
    /// # Warning
    ///
    /// When sets `true`, any valid certificate for any site will be trusted for
    /// use from any other.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().danger_accept_invalid_hostnames(true);
    /// ```
    pub fn danger_accept_invalid_hostnames(mut self, is_invalid: bool) -> Self {
        self.tls = self.tls.danger_accept_invalid_hostnames(is_invalid);
        self
    }

    /// Controls the use of TLS server name indication.
    ///
    /// Defaults to `true` -- sets sni.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let builder = ClientBuilder::new().tls_sni(true);
    /// ```
    pub fn tls_sni(mut self, is_set_sni: bool) -> Self {
        self.tls = self.tls.sni(is_set_sni);
        self
    }

    /// Controls the use of TLS certs verifier.
    ///
    /// Defaults to `None` -- sets cert_verifier.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::{CertVerifier, ServerCerts};
    ///
    /// pub struct CallbackTest {
    ///     inner: String,
    /// }
    ///
    /// impl CallbackTest {
    ///     pub(crate) fn new() -> Self {
    ///         Self {
    ///             inner: "Test".to_string(),
    ///         }
    ///     }
    /// }
    ///
    /// impl CertVerifier for CallbackTest {
    ///     fn verify(&self, certs: &ServerCerts) -> bool {
    ///         true
    ///     }
    /// }
    ///
    /// let verifier = CallbackTest::new();
    /// let builder = ClientBuilder::new().cert_verifier(verifier);
    /// ```
    pub fn cert_verifier<T: CertVerifier + Send + Sync + 'static>(mut self, verifier: T) -> Self {
        use std::sync::Arc;

        use crate::util::config::tls::DefaultCertVerifier;

        self.tls = self
            .tls
            .cert_verifier(Arc::new(DefaultCertVerifier::new(verifier)));
        self
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
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    use crate::async_impl::{Body, Request, Response};
    use crate::Proxy;
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    use crate::{CertVerifier, ServerCerts};

    /// UT test cases for `Client::builder`.
    ///
    /// # Brief
    /// 1. Creates a ClientBuilder by calling `Client::Builder`.
    /// 2. Calls `http_config`, `client_config`, `build` on the builder
    ///    respectively.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "http1_1")]
    #[test]
    fn ut_client_builder() {
        let builder = Client::builder().http1_only().build();
        assert!(builder.is_ok());
        let builder_proxy = Client::builder()
            .proxy(Proxy::http("http://www.example.com").build().unwrap())
            .build();
        assert!(builder_proxy.is_ok());
    }

    /// UT test cases for `ClientBuilder::default`.
    ///
    /// # Brief
    /// 1. Creates a `ClientBuilder` by calling `ClientBuilder::default`.
    /// 2. Calls `http_config`, `client_config`, `tls_config` and `build`
    ///    respectively.
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

    /// UT test cases for `ClientBuilder::default`.
    ///
    /// # Brief
    /// 1. Creates a `ClientBuilder` by calling `ClientBuilder::default`.
    /// 2. Set redirect for client and call `Client::redirect_request`.
    /// 3. Checks if the result is as expected.
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    #[test]
    fn ut_client_request_redirect() {
        let handle = ylong_runtime::spawn(async move {
            client_request_redirect().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    async fn client_request_redirect() {
        use ylong_http::h1::ResponseDecoder;
        use ylong_http::response::Response as HttpResponse;

        use crate::async_impl::{ClientBuilder, HttpBody};
        use crate::util::normalizer::BodyLength;
        use crate::util::{Redirect, Timeout};

        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nLocation: \t http://example3.com:80/foo?a=1 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();

        let box_stream = Box::new("hello world".as_bytes());
        let content_bytes = "";
        let until_close =
            HttpBody::new(BodyLength::UntilClose, box_stream, content_bytes.as_bytes()).unwrap();
        let response = HttpResponse::from_raw_parts(result.0, until_close);
        let response = Response::new(response);
        let mut request = Request::builder()
            .url("http://example1.com:80/foo?a=1")
            .body(Body::slice("this is a body"))
            .unwrap();

        let client = ClientBuilder::default()
            .redirect(Redirect::limited(2))
            .connect_timeout(Timeout::from_secs(2))
            .build()
            .unwrap();
        let res = client.redirect(response, &mut request).await;
        assert!(res.is_ok())
    }

    /// UT test cases for `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Set version HTTP/1.0 for client and call `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    #[test]
    fn ut_client_request_http1_0() {
        let handle = ylong_runtime::spawn(async move {
            client_request_version_1_0().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    async fn client_request_version_1_0() {
        let request = Request::builder()
            .url("http://example1.com:80/foo?a=1")
            .method("CONNECT")
            .version("HTTP/1.0")
            .body(Body::empty())
            .unwrap();

        let client = Client::builder().http1_only().build().unwrap();
        let res = client.request(request).await;
        assert!(res
            .map_err(|e| {
                assert_eq!(format!("{e}"), "Request Error: Unknown METHOD in HTTP/1.0");
                e
            })
            .is_err());
    }

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    #[test]
    fn ut_client_request_verify() {
        let handle = ylong_runtime::spawn(async move {
            client_request_verify().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    struct Verifier;

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    impl CertVerifier for Verifier {
        fn verify(&self, _certs: &ServerCerts) -> bool {
            false
        }
    }

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    async fn client_request_verify() {
        // Creates a `async_impl::Client`
        let client = Client::builder().cert_verifier(Verifier).build().unwrap();
        // Creates a `Request`.
        let request = Request::builder()
            .url("https://www.example.com")
            .body(Body::empty())
            .unwrap();
        // Sends request and receives a `Response`.
        let response = client.request(request).await;
        assert!(response.is_err())
    }
}
