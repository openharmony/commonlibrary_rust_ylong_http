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

use std::sync::Arc;

use ylong_http::request::uri::Uri;

use super::pool::ConnPool;
use super::timeout::TimeoutFuture;
use super::{conn, Body, Connector, HttpConnector, Request, Response};
use crate::async_impl::interceptor::{IdleInterceptor, Interceptor, Interceptors};
use crate::async_impl::request::Message;
use crate::error::HttpClientError;
use crate::runtime::timeout;
#[cfg(feature = "__c_openssl")]
use crate::util::c_openssl::verify::PubKeyPins;
use crate::util::config::{
    ClientConfig, ConnectorConfig, HttpConfig, HttpVersion, Proxy, Redirect, Timeout,
};
use crate::util::dispatcher::Conn;
use crate::util::normalizer::RequestFormatter;
use crate::util::proxy::Proxies;
use crate::util::redirect::{RedirectInfo, Trigger};
use crate::util::request::RequestArc;
#[cfg(feature = "__c_openssl")]
use crate::CertVerifier;
use crate::Retry;

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
    interceptors: Arc<Interceptors>,
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
            interceptors: Arc::new(IdleInterceptor),
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
        let mut request = RequestArc::new(request);
        let mut retries = self.config.retry.times().unwrap_or(0);
        loop {
            let response = self.send_request(request.clone()).await;
            if let Err(ref err) = response {
                if retries > 0 && request.ref_mut().body_mut().reuse() {
                    self.interceptors.intercept_retry(err)?;
                    retries -= 1;
                    continue;
                }
            }
            return response;
        }
    }
}

impl<C: Connector> Client<C> {
    async fn send_request(&self, request: RequestArc) -> Result<Response, HttpClientError> {
        let response = self.send_unformatted_request(request.clone()).await?;
        self.redirect(response, request).await
    }

    async fn send_unformatted_request(
        &self,
        mut request: RequestArc,
    ) -> Result<Response, HttpClientError> {
        RequestFormatter::new(request.ref_mut()).format()?;
        let conn = self.connect_to(request.ref_mut().uri()).await?;
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
        request: RequestArc,
    ) -> Result<Response, HttpClientError> {
        let message = Message {
            request,
            interceptor: Arc::clone(&self.interceptors),
        };
        if let Some(timeout) = self.config.request_timeout.inner() {
            TimeoutFuture::new(conn::request(conn, message), timeout).await
        } else {
            conn::request(conn, message).await
        }
    }

    async fn redirect(
        &self,
        response: Response,
        mut request: RequestArc,
    ) -> Result<Response, HttpClientError> {
        let mut response = response;
        let mut info = RedirectInfo::new();
        loop {
            match self
                .config
                .redirect
                .inner()
                .redirect(request.ref_mut(), &response, &mut info)?
            {
                Trigger::NextLink => {
                    // Here the body should be reused.
                    if !request.ref_mut().body_mut().reuse() {
                        *request.ref_mut().body_mut() = Body::empty();
                    }
                    self.interceptors
                        .intercept_redirect_request(request.ref_mut())?;
                    response = self.send_unformatted_request(request.clone()).await?;
                    self.interceptors.intercept_redirect_response(&response)?;
                }
                Trigger::Stop => {
                    self.interceptors.intercept_response(&response)?;
                    return Ok(response);
                }
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

    interceptors: Arc<Interceptors>,

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
            interceptors: Arc::new(IdleInterceptor),
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

    /// Sets retry times for this client.
    ///
    /// The Retry is the number of times the client will retry the request if
    /// the response is not obtained correctly.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::Retry;
    ///
    /// let builder = ClientBuilder::new().retry(Retry::max());
    /// ```
    pub fn retry(mut self, retry: Retry) -> Self {
        self.client.retry = retry;
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

    /// Adds a `Interceptor` to the `Client`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{ClientBuilder, Interceptor};
    /// # use ylong_http_client::HttpClientError;
    ///
    /// # fn add_interceptor<T>(interceptor: T)
    /// # where T: Interceptor + Sync + Send + 'static,
    /// # {
    /// let builder = ClientBuilder::new().interceptor(interceptor);
    /// # }
    /// ```
    pub fn interceptor<T>(mut self, interceptors: T) -> Self
    where
        T: Interceptor + Sync + Send + 'static,
    {
        self.interceptors = Arc::new(interceptors);
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
        #[cfg(feature = "__c_openssl")]
        use crate::util::{AlpnProtocol, AlpnProtocolList};

        #[cfg(feature = "__c_openssl")]
        let origin_builder = self.tls;
        #[cfg(feature = "__c_openssl")]
        let tls_builder = match self.http.version {
            HttpVersion::Http1 => origin_builder,
            #[cfg(feature = "http2")]
            HttpVersion::Http2 => origin_builder.alpn_protos(AlpnProtocol::H2.wire_format_bytes()),
            HttpVersion::Negotiate => {
                let supported = AlpnProtocolList::new();
                #[cfg(feature = "http2")]
                let supported = supported.extend(AlpnProtocol::H2);
                let supported = supported.extend(AlpnProtocol::HTTP11);
                origin_builder.alpn_proto_list(supported)
            }
        };

        let config = ConnectorConfig {
            proxies: self.proxies,
            #[cfg(feature = "__tls")]
            tls: tls_builder.build()?,
        };

        let connector = HttpConnector::new(config);

        Ok(Client {
            inner: ConnPool::new(self.http, connector),
            config: self.client,
            interceptors: self.interceptors,
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
        self.http.version = HttpVersion::Http2;
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
        self.http.http2_config.set_max_frame_size(size);
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
        self.http.http2_config.set_max_header_list_size(size);
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
        self.http.http2_config.set_header_table_size(size);
        self
    }

    /// Sets the maximum connection window allowed by the client.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let config = ClientBuilder::new().set_conn_recv_window_size(4096);
    /// ```
    pub fn set_conn_recv_window_size(mut self, size: u32) -> Self {
        assert!(size <= crate::util::h2::MAX_FLOW_CONTROL_WINDOW);
        self.http.http2_config.set_conn_window_size(size);
        self
    }

    /// Sets the `SETTINGS_INITIAL_WINDOW_SIZE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    ///
    /// let config = ClientBuilder::new().set_stream_recv_window_size(4096);
    /// ```
    pub fn set_stream_recv_window_size(mut self, size: u32) -> Self {
        assert!(size <= crate::util::h2::MAX_FLOW_CONTROL_WINDOW);
        self.http.http2_config.set_stream_window_size(size);
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

    /// Adds user pinned Public Key.
    ///
    /// Used to avoid man-in-the-middle attacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::ClientBuilder;
    /// use ylong_http_client::PubKeyPins;
    ///
    /// let pinned_key = PubKeyPins::builder()
    /// .add("https://example.com:443",
    /// "sha256//YhKJKSzoTt2b5FP18fvpHo7fJYqQCjAa3HWY3tvRMwE=;sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=")
    /// .build()
    /// .unwrap();
    /// let builder = ClientBuilder::new().add_public_key_pins(pinned_key);
    /// ```
    pub fn add_public_key_pins(mut self, pin: PubKeyPins) -> Self {
        self.tls = self.tls.pinning_public_key(pin);
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
    #[cfg(feature = "ylong_base")]
    use ylong_runtime::io::AsyncWriteExt;

    #[cfg(feature = "ylong_base")]
    use crate::async_impl::{Body, Request, Response};
    use crate::async_impl::{Client, HttpConnector};
    #[cfg(feature = "ylong_base")]
    use crate::util::test_utils::{format_header_str, TcpHandle};
    #[cfg(feature = "ylong_base")]
    use crate::{build_client_request, start_tcp_server, Retry};
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    use crate::{CertVerifier, ServerCerts};
    #[cfg(feature = "__tls")]
    use crate::{Certificate, TlsVersion};
    use crate::{Proxy, Timeout};

    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    struct Verifier;

    #[cfg(feature = "ylong_base")]
    async fn client_request_redirect() {
        use std::sync::Arc;

        use ylong_http::h1::ResponseDecoder;
        use ylong_http::response::Response as HttpResponse;

        use crate::async_impl::interceptor::IdleInterceptor;
        use crate::async_impl::{ClientBuilder, HttpBody};
        use crate::util::normalizer::BodyLength;
        use crate::util::request::RequestArc;
        use crate::util::Redirect;

        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nLocation: \t http://example3.com:80/foo?a=1 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();

        let box_stream = Box::new("hello world".as_bytes());
        let content_bytes = "";
        let until_close = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::UntilClose,
            box_stream,
            content_bytes.as_bytes(),
        )
        .unwrap();
        let response = HttpResponse::from_raw_parts(result.0, until_close);
        let response = Response::new(response);
        let request = Request::builder()
            .url("http://example1.com:80/foo?a=1")
            .body(Body::slice("this is a body"))
            .unwrap();
        let request = RequestArc::new(request);

        let client = ClientBuilder::default()
            .redirect(Redirect::limited(2))
            .connect_timeout(Timeout::from_secs(2))
            .build()
            .unwrap();
        let res = client.redirect(response, request.clone()).await;
        assert!(res.is_ok())
    }

    #[cfg(feature = "ylong_base")]
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
    impl CertVerifier for Verifier {
        fn verify(&self, certs: &ServerCerts) -> bool {
            // get version
            let _v = certs.version().unwrap();
            // get issuer
            let _i = certs.issuer().unwrap();
            // get name
            let _n = certs.cert_name().unwrap();
            // cmp cert file
            let cert_pem = r#"-----BEGIN CERTIFICATE-----
MIIDGzCCAgMCCQCHcfe97pgvpTANBgkqhkiG9w0BAQsFADBFMQswCQYDVQQGEwJB
VTETMBEGA1UECAwKU29tZS1TdGF0ZTEhMB8GA1UECgwYSW50ZXJuZXQgV2lkZ2l0
cyBQdHkgTHRkMB4XDTE2MDgxNDE3MDAwM1oXDTI2MDgxMjE3MDAwM1owWjELMAkG
A1UEBhMCQVUxEzARBgNVBAgMClNvbWUtU3RhdGUxITAfBgNVBAoMGEludGVybmV0
IFdpZGdpdHMgUHR5IEx0ZDETMBEGA1UEAwwKZm9vYmFyLmNvbTCCASIwDQYJKoZI
hvcNAQEBBQADggEPADCCAQoCggEBAKj0JYxEsxejUIX+I5GH0Hg2G0kX/y1H0+Ub
3mw2/Ja5BD/yN96/7zMSumXF8uS3SkmpyiJkbyD01TSRTqjlP7/VCBlyUIChlpLQ
mrGaijZiT/VCyPXqmcwFzXS5IOTpX1olJfW8rA41U1LCIcDUyFf6LtZ/v8rSeKr6
TuE6SGV4WRaBm1SrjWBeHVV866CRrtSS1ieT2asFsAyOZqWhk2fakwwBDFWDhOGI
ubfO+5aq9cBJbNRlzsgB3UZs3gC0O6GzbnZ6oT0TiJMeTsXXjABLUlaq/rrqFF4Y
euZkkbHTFBMz288PUc3m3ZTcpN+E7+ZOUBRZXKD20K07NugqCzUCAwEAATANBgkq
hkiG9w0BAQsFAAOCAQEASvYHuIl5C0NHBELPpVHNuLbQsDQNKVj3a54+9q1JkiMM
6taEJYfw7K1Xjm4RoiFSHpQBh+PWZS3hToToL2Zx8JfMR5MuAirdPAy1Sia/J/qE
wQdJccqmvuLkLTSlsGbEJ/LUUgOAgrgHOZM5lUgIhCneA0/dWJ3PsN0zvn69/faY
oo1iiolWiIHWWBUSdr3jM2AJaVAsTmLh00cKaDNk37JB940xConBGSl98JPrNrf9
dUAiT0iIBngDBdHnn/yTj+InVEFyZSKrNtiDSObFHxPcxGteHNrCPJdP1e+GqkHp
HJMRZVCQpSMzvHlofHSNgzWV1MX5h1CP4SGZdBDTfA==
-----END CERTIFICATE-----"#;
            let _c = certs.cmp_pem_cert(cert_pem.as_bytes()).unwrap();
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

    /// UT test cases for `Client::with_connector`.
    ///
    /// # Brief
    /// 1. Creates a Client by calling `Client::with_connector`.
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_client_with_connector() {
        let client = Client::with_connector(HttpConnector::default());
        assert_eq!(client.config.connect_timeout, Timeout::none())
    }

    /// UT test cases for `Client::new`.
    ///
    /// # Brief
    /// 1. Creates a Client by calling `Client::new`.
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_client_new() {
        let client = Client::new();
        assert_eq!(client.config.connect_timeout, Timeout::none())
    }

    /// UT test cases for `Client::default`.
    ///
    /// # Brief
    /// 1. Creates a Client by calling `Client::default`.
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_client_default() {
        let client = Client::default();
        assert_eq!(client.config.connect_timeout, Timeout::none())
    }

    /// UT test cases for `ClientBuilder::build`.
    ///
    /// # Brief
    /// 1. Creates a ClientBuilder by calling `Client::Builder`.
    /// 2. Checks if the result is as expected.
    #[cfg(feature = "__tls")]
    #[test]
    fn ut_client_build_tls() {
        let client = Client::builder()
            .max_tls_version(TlsVersion::TLS_1_3)
            .min_tls_version(TlsVersion::TLS_1_0)
            .add_root_certificate(Certificate::from_pem(b"cert").unwrap())
            .tls_ca_file("ca.crt")
            .tls_cipher_list(
                "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK",
            )
            .tls_cipher_suites(
                "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK",
            )
            .tls_built_in_root_certs(false)
            .danger_accept_invalid_certs(false)
            .danger_accept_invalid_hostnames(false)
            .tls_sni(false)
            .build();

        assert!(client.is_err());
    }

    /// UT test cases for `ClientBuilder::build`.
    ///
    /// # Brief
    /// 1. Creates a ClientBuilder by calling `Client::Builder`.
    /// 2. Checks if the result is as expected.
    #[cfg(feature = "__tls")]
    #[test]
    fn ut_client_build_tls_pubkey_pinning() {
        use crate::PubKeyPins;

        let client = Client::builder()
            .tls_built_in_root_certs(true) // not use root certs
            .danger_accept_invalid_certs(true) // not verify certs
            .max_tls_version(TlsVersion::TLS_1_2)
            .min_tls_version(TlsVersion::TLS_1_2)
            .add_public_key_pins(
                PubKeyPins::builder()
                    .add(
                        "https://7.249.243.101:6789",
                        "sha256//VHQAbNl67nmkZJNESeTKvTxb5bQmd1maWnMKG/tjcAY=",
                    )
                    .build()
                    .unwrap(),
            )
            .build();
        assert!(client.is_ok())
    }

    /// UT test cases for `ClientBuilder::default`.
    ///
    /// # Brief
    /// 1. Creates a `ClientBuilder` by calling `ClientBuilder::default`.
    /// 2. Calls `http_config`, `client_config`, `tls_config` and `build`
    ///    respectively.
    /// 3. Checks if the result is as expected.
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
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_request_redirect() {
        let handle = ylong_runtime::spawn(async move {
            client_request_redirect().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Set version HTTP/1.0 for client and call `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_connect_http1_0() {
        let handle = ylong_runtime::spawn(async move {
            client_request_version_1_0().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for retry of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Set version HTTP/1.0 for client and call `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_request_http1_0_retry() {
        let request = Request::builder()
            .url("http://example1.com:80/foo?a=1")
            .method("CONNECT")
            .version("HTTP/1.0")
            .body(Body::empty())
            .unwrap();

        let retry_times = Retry::new(1).unwrap();
        let client = Client::builder()
            .retry(retry_times)
            .http1_only()
            .build()
            .unwrap();

        let handle = ylong_runtime::spawn(async move {
            let res = client.request(request).await;
            assert!(res
                .map_err(|e| {
                    assert_eq!(format!("{e}"), "Request Error: Unknown METHOD in HTTP/1.0");
                    e
                })
                .is_err());
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for certificate verify of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. implement `CertVerifier` for struct `Verifier`.
    /// 3. Sets `CertVerifier` for this client.
    /// 4. Checks if the result is as expected.
    #[cfg(all(feature = "__tls", feature = "ylong_base"))]
    #[test]
    fn ut_client_request_verify() {
        let handle = ylong_runtime::spawn(async move {
            client_request_verify().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for certificate verify of `Client::send_request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::send_request`.
    /// 4. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_send_request() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder()
            .connect_timeout(Timeout::from_secs(2))
            .http1_only()
            .build()
            .unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            let body = resp.unwrap().text().await;
            assert!(body.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for retry of `Client::connect_to`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sets connect timeout for this client.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_connect_to() {
        let client = Client::builder()
            .connect_timeout(Timeout::from_secs(1))
            .http1_only()
            .build()
            .unwrap();

        let request = build_client_request!(
            Request: {
                Path: "",
                Addr: "198.18.0.25:80",
                Body: Body::empty(),
            },
        );
        let handle = ylong_runtime::spawn(async move {
            let res = client.request(request).await;
            assert!(res.is_err());
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for certificate verify of `Client::redirect`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::redirect`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_redirect() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 302,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Header: "Location", "http://ylong_http.com:80",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder()
            .request_timeout(Timeout::from_secs(2))
            .http1_only()
            .build()
            .unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_err());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for proxy of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_http_proxy() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: "ylong_http.com",
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder()
            .proxy(
                Proxy::http(format!("http://{}{}", handle.addr.as_str(), "/data").as_str())
                    .build()
                    .expect("Http proxy build failed"),
            )
            .build()
            .expect("Client build failed!");

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for sends chunk body of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_send_trunk_body() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Transfer-Encoding", "chunked",
                Body: Body::slice("aaaaa bbbbb ccccc ddddd".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for sends no headers request of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_send_unknown_size() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Body: Body::empty(),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for receive `Connection` header response of
    /// `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_recv_conn_close() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Header: "Connection", "close",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for receive HTTP/1.0 response with invalid header of
    /// `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_recv_http1_0_resp() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.0",
               Header: "Content-Length", "11",
               Header: "Connection", "close",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Version: "HTTP/1.0",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_ok());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for receive HTTP/1.0 response with transfer-encoding
    /// header of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_recv_invalid_http1_0_resp() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 201,
               Version: "HTTP/1.0",
               Header: "Transfer-Encoding", "chunked",
               Body: "0\r\n\r\n",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Version: "HTTP/1.0",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_err());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for receive response when server is shutdown of
    /// `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_recv_when_server_shutdown() {
        let mut handles = vec![];
        start_tcp_server!(Handles: handles, Shutdown: std::net::Shutdown::Both,);
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_err());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for receive response status in error of `Client::request`.
    ///
    /// # Brief
    /// 1. Creates a `Client` by calling `Client::builder()`.
    /// 2. Sends a `Request` by `Client::request`.
    /// 3. Checks if the result is as expected.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_recv_error_resp_status() {
        let mut handles = vec![];
        start_tcp_server!(
           Handles: handles,
           Response: {
               Status: 2023,
               Version: "HTTP/1.1",
               Header: "Content-Length", "11",
               Header: "Connection", "close",
               Body: "METHOD GET!",
           },
        );
        let handle = handles.pop().expect("No more handles !");

        let request = build_client_request!(
            Request: {
                Method: "GET",
                Path: "/data",
                Addr: handle.addr.as_str(),
                Header: "Content-Length", "5",
                Body: Body::slice("HELLO".as_bytes()),
            },
        );
        let client = Client::builder().http1_only().build().unwrap();

        let handle = ylong_runtime::spawn(async move {
            let resp = client.request(request).await;
            assert!(resp.is_err());
            handle
                .server_shutdown
                .recv()
                .expect("server send order failed !");
        });
        ylong_runtime::block_on(handle).unwrap();
    }
}
