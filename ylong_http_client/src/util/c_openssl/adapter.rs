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

use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use crate::error::{ErrorKind, HttpClientError};
use crate::util::c_openssl::error::ErrorStack;
use crate::util::c_openssl::ssl::{
    Ssl, SslContext, SslContextBuilder, SslFiletype, SslMethod, SslVersion,
};
use crate::util::c_openssl::verify::PubKeyPins;
use crate::util::c_openssl::x509::{X509Store, X509};
use crate::util::config::tls::DefaultCertVerifier;
use crate::util::AlpnProtocolList;

/// `TlsContextBuilder` implementation based on `SSL_CTX`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::{TlsConfigBuilder, TlsVersion};
///
/// let context = TlsConfigBuilder::new()
///     .ca_file("ca.crt")
///     .max_proto_version(TlsVersion::TLS_1_2)
///     .min_proto_version(TlsVersion::TLS_1_2)
///     .cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
///     .build();
/// ```
pub struct TlsConfigBuilder {
    inner: Result<SslContextBuilder, ErrorStack>,
    cert_verifier: Option<Arc<DefaultCertVerifier>>,
    use_sni: bool,
    verify_hostname: bool,
    certs_list: Vec<Cert>,
    pins: Option<PubKeyPins>,
    #[cfg(feature = "c_openssl_3_0")]
    paths_list: Vec<String>,
}

impl TlsConfigBuilder {
    /// Creates a new, default `SslContextBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            inner: SslContext::builder(SslMethod::tls_client()),
            cert_verifier: None,
            use_sni: true,
            verify_hostname: true,
            certs_list: vec![],
            pins: None,
            #[cfg(feature = "c_openssl_3_0")]
            paths_list: vec![],
        }
    }

    /// Loads trusted root certificates from a file. The file should contain a
    /// sequence of PEM-formatted CA certificates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().ca_file("ca.crt");
    /// ```
    pub fn ca_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_ca_file(path).map(|_| builder));
        self
    }

    /// Sets the maximum supported protocol version. A value of `None` will
    /// enable protocol versions down the highest version supported by
    /// `OpenSSL`.
    ///
    /// Requires `OpenSSL 1.1.0` or `LibreSSL 2.6.1` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{TlsConfigBuilder, TlsVersion};
    ///
    /// let builder = TlsConfigBuilder::new().max_proto_version(TlsVersion::TLS_1_2);
    /// ```
    pub fn max_proto_version(mut self, version: TlsVersion) -> Self {
        self.inner = self.inner.and_then(|mut builder| {
            builder
                .set_max_proto_version(version.into_inner())
                .map(|_| builder)
        });
        self
    }

    /// Sets the minimum supported protocol version. A value of `None` will
    /// enable protocol versions down the the lowest version supported by
    /// `OpenSSL`.
    ///
    /// Requires `OpenSSL 1.1.0` or `LibreSSL 2.6.1` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{TlsConfigBuilder, TlsVersion};
    ///
    /// let builder = TlsConfigBuilder::new().min_proto_version(TlsVersion::TLS_1_2);
    /// ```
    pub fn min_proto_version(mut self, version: TlsVersion) -> Self {
        self.inner = self.inner.and_then(|mut builder| {
            builder
                .set_min_proto_version(version.into_inner())
                .map(|_| builder)
        });
        self
    }

    /// Sets the list of supported ciphers for protocols before `TLSv1.3`.
    ///
    /// The `set_ciphersuites` method controls the cipher suites for `TLSv1.3`.
    ///
    /// See [`ciphers`] for details on the format.
    ///
    /// [`ciphers`]: https://www.openssl.org/docs/man1.1.0/apps/ciphers.html
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK");
    /// ```
    pub fn cipher_list(mut self, list: &str) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_cipher_list(list).map(|_| builder));
        self
    }

    /// Sets the list of supported ciphers for the `TLSv1.3` protocol.
    ///
    /// The `set_cipher_list` method controls the cipher suites for protocols
    /// before `TLSv1.3`.
    ///
    /// The format consists of TLSv1.3 cipher suite names separated by `:`
    /// characters in order of preference.
    ///
    /// Requires `OpenSSL 1.1.1` or `LibreSSL 3.4.0` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .cipher_suites("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK");
    /// ```
    pub fn cipher_suites(mut self, list: &str) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_cipher_suites(list).map(|_| builder));
        self
    }

    /// Loads a leaf certificate from a file.
    ///
    /// Only a single certificate will be loaded - use `add_extra_chain_cert` to
    /// add the remainder of the certificate chain, or
    /// `set_certificate_chain_file` to load the entire chain from a single
    /// file.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{TlsConfigBuilder, TlsFileType};
    ///
    /// let builder = TlsConfigBuilder::new().certificate_file("cert.pem", TlsFileType::PEM);
    /// ```
    pub fn certificate_file<T: AsRef<Path>>(mut self, path: T, file_type: TlsFileType) -> Self {
        self.inner = self.inner.and_then(|mut builder| {
            builder
                .set_certificate_file(path, file_type.into_inner())
                .map(|_| builder)
        });
        self
    }

    /// Loads a certificate chain from a file.
    ///
    /// The file should contain a sequence of PEM-formatted certificates,
    /// the first being the leaf certificate, and the remainder forming the
    /// chain of certificates up to and including the trusted root certificate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().certificate_chain_file("cert.pem");
    /// ```
    pub fn certificate_chain_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_certificate_chain_file(path).map(|_| builder));
        self
    }

    /// Adds custom root certificate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{Cert, TlsConfigBuilder};
    /// # fn example(certs: Vec<Cert>) {
    /// let builder = TlsConfigBuilder::new().add_root_certificates(certs);
    /// # }
    /// ```
    pub fn add_root_certificates(mut self, mut certs: Vec<Cert>) -> Self {
        self.certs_list.append(&mut certs);
        self
    }

    /// Adds custom root certificate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    /// # fn example(path: String) {
    /// let builder = TlsConfigBuilder::new().add_path_certificates(path);
    /// # }
    /// ```
    #[cfg(feature = "c_openssl_3_0")]
    pub fn add_path_certificates(mut self, path: String) -> Self {
        self.paths_list.push(path);
        self
    }

    // Sets the protocols to sent to the server for Application Layer Protocol
    // Negotiation (ALPN).
    //
    // Requires OpenSSL 1.0.2 or LibreSSL 2.6.1 or newer.
    #[cfg(feature = "http2")]
    pub(crate) fn alpn_protos(mut self, protocols: &[u8]) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_alpn_protos(protocols).map(|_| builder));
        self
    }

    // Sets the protocols to sent to the server for Application Layer Protocol
    // Negotiation (ALPN).
    //
    // This method is based on `openssl::SslContextBuilder::set_alpn_protos`.
    // Requires OpenSSL 1.0.2 or LibreSSL 2.6.1 or newer.
    pub(crate) fn alpn_proto_list(mut self, list: AlpnProtocolList) -> Self {
        self.inner = self
            .inner
            .and_then(|mut builder| builder.set_alpn_protos(list.as_slice()).map(|_| builder));
        self
    }

    /// Controls the use of built-in system certificates during certificate
    /// validation. Default to `true` -- uses built-in system certs.
    pub fn build_in_root_certs(mut self, is_use: bool) -> Self {
        if !is_use {
            self.inner = X509Store::new().and_then(|store| {
                self.inner.and_then(|mut builder| {
                    {
                        builder.set_cert_store(store);
                        Ok(())
                    }
                    .map(|_| builder)
                })
            });
        }
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
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().danger_accept_invalid_certs(true);
    /// ```
    pub fn danger_accept_invalid_certs(mut self, is_invalid: bool) -> Self {
        if is_invalid {
            self.inner = self.inner.and_then(|mut builder| {
                {
                    builder.set_verify(crate::util::c_openssl::ssl::SSL_VERIFY_NONE);
                    Ok(())
                }
                .map(|_| builder)
            });
        }
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
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().danger_accept_invalid_hostnames(true);
    /// ```
    pub fn danger_accept_invalid_hostnames(mut self, invalid_hostname: bool) -> Self {
        self.verify_hostname = !invalid_hostname;
        self
    }

    pub(crate) fn cert_verifier(mut self, verifier: Arc<DefaultCertVerifier>) -> Self {
        let inner = Arc::as_ptr(&verifier);
        self.cert_verifier = Some(verifier);
        self.inner = self.inner.map(|mut builder| {
            builder.set_cert_verify_callback(inner);
            builder
        });
        self
    }

    pub(crate) fn pinning_public_key(mut self, pin: PubKeyPins) -> Self {
        self.pins = Some(pin);
        self
    }

    /// Controls the use of TLS server name indication.
    ///
    /// Defaults to `true` -- sets sni.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().sni(true);
    /// ```
    pub fn sni(mut self, use_sni: bool) -> Self {
        self.use_sni = use_sni;
        self
    }

    /// Builds a `TlsContext`. Returns `Err` if an error occurred during
    /// configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{TlsConfigBuilder, TlsVersion};
    ///
    /// let context = TlsConfigBuilder::new()
    ///     .ca_file("ca.crt")
    ///     .max_proto_version(TlsVersion::TLS_1_2)
    ///     .min_proto_version(TlsVersion::TLS_1_2)
    ///     .cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
    ///     .build();
    /// ```
    pub fn build(mut self) -> Result<TlsConfig, HttpClientError> {
        for cert in self.certs_list {
            self.inner = self.inner.and_then(|mut builder| {
                { Ok(builder.cert_store_mut()).map(|store| store.add_cert(cert.0)) }
                    .map(|_| builder)
            });
        }

        #[cfg(feature = "c_openssl_3_0")]
        for path in self.paths_list {
            self.inner = self.inner.and_then(|mut builder| {
                { Ok(builder.cert_store_mut()).map(|store| store.add_path(path)) }.map(|_| builder)
            });
        }

        let ctx = self
            .inner
            .map(|builder| builder.build())
            .map_err(|e| HttpClientError::from_error(ErrorKind::Build, e))?;

        Ok(TlsConfig {
            ctx,
            cert_verifier: self.cert_verifier,
            use_sni: self.use_sni,
            verify_hostname: self.verify_hostname,
            pins: self.pins,
        })
    }
}

impl Default for TlsConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// `TlsContext` is based on `SSL_CTX`, which provides context
/// object of `TLS` streams.
///
/// # Examples
///
/// ```
/// use ylong_http_client::TlsConfig;
///
/// let builder = TlsConfig::builder();
/// ```
#[derive(Clone)]
pub struct TlsConfig {
    ctx: SslContext,
    #[allow(dead_code)]
    cert_verifier: Option<Arc<DefaultCertVerifier>>,
    use_sni: bool,
    verify_hostname: bool,
    pins: Option<PubKeyPins>,
}

impl TlsConfig {
    /// Creates a new, default `TlsContextBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::TlsConfig;
    ///
    /// let builder = TlsConfig::builder();
    /// ```
    pub fn builder() -> TlsConfigBuilder {
        TlsConfigBuilder::new()
    }

    /// Creates a new, default `TlsSsl`.
    pub(crate) fn ssl_new(&self, domain: &str) -> Result<TlsSsl, ErrorStack> {
        let ctx = &self.ctx;
        let mut ssl = Ssl::new(ctx)?;

        // SNI extension in `ClientHello` stage.
        if self.use_sni && domain.parse::<IpAddr>().is_err() {
            ssl.set_host_name_in_sni(domain)?;
        }

        // Hostname verification in certificate verification.
        if self.verify_hostname {
            ssl.set_verify_hostname(domain)?;
        }
        Ok(TlsSsl(ssl))
    }

    pub(crate) fn pinning_host_match(&self, domain: &str) -> Option<String> {
        match &self.pins {
            None => None,
            Some(pins) => pins.get_pin(domain),
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        // It certainly can be successful.
        TlsConfig::builder()
            .build()
            .expect("TlsConfig build error!")
    }
}

/// /// `TlsSsl` is based on `Ssl`
pub(crate) struct TlsSsl(Ssl);

impl TlsSsl {
    pub(crate) fn into_inner(self) -> Ssl {
        self.0
    }
}

/// `TlsVersion` is based on `openssl::SslVersion`, which provides `SSL/TLS`
/// protocol version.
///
/// # Examples
///
/// ```
/// use ylong_http_client::TlsVersion;
///
/// let version = TlsVersion::TLS_1_2;
/// ```
pub struct TlsVersion(SslVersion);

impl TlsVersion {
    /// Constant for TLS version 1.
    pub const TLS_1_0: Self = Self(SslVersion::TLS_1_0);
    /// Constant for TLS version 1.1.
    pub const TLS_1_1: Self = Self(SslVersion::TLS_1_1);
    /// Constant for TLS version 1.2.
    pub const TLS_1_2: Self = Self(SslVersion::TLS_1_2);
    /// Constant for TLS version 1.3.
    pub const TLS_1_3: Self = Self(SslVersion::TLS_1_3);

    /// Consumes `TlsVersion` and then takes `SslVersion`.
    pub(crate) fn into_inner(self) -> SslVersion {
        self.0
    }
}

/// `TlsFileType` is based on `openssl::SslFileType`, which provides an
/// identifier of the format of a certificate or key file.
///
/// ```
/// use ylong_http_client::TlsFileType;
///
/// let file_type = TlsFileType::PEM;
/// ```
pub struct TlsFileType(SslFiletype);

impl TlsFileType {
    /// Constant for PEM file type.
    pub const PEM: Self = Self(SslFiletype::PEM);
    /// Constant for ASN1 file type.
    pub const ASN1: Self = Self(SslFiletype::ASN1);

    /// Consumes `TlsFileType` and then takes `SslFiletype`.
    pub(crate) fn into_inner(self) -> SslFiletype {
        self.0
    }
}

/// `Cert` is based on `X509`, which indicates `X509` public
/// key certificate.
///
/// ```
/// # use ylong_http_client::Cert;
///
/// # fn read_from_pem(pem: &[u8]) {
/// let cert = Cert::from_pem(pem);
/// # }
///
/// # fn read_from_der(der: &[u8]) {
/// let cert = Cert::from_der(der);
/// # }
/// ```
#[derive(Clone)]
pub struct Cert(X509);

impl Cert {
    /// Deserializes a PEM-encoded `Cert` structure.
    ///
    /// The input should have a header like below:
    ///
    /// ```text
    /// -----BEGIN CERTIFICATE-----
    /// ```
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::Cert;
    ///
    /// # fn read_from_pem(pem: &[u8]) {
    /// let cert = Cert::from_pem(pem);
    /// # }
    /// ```
    pub fn from_pem(pem: &[u8]) -> Result<Self, HttpClientError> {
        Ok(Self(X509::from_pem(pem).map_err(|e| {
            HttpClientError::from_error(ErrorKind::Build, e)
        })?))
    }

    /// Deserializes a DER-encoded `Cert` structure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Cert;
    ///
    /// # fn read_from_der(der: &[u8]) {
    /// let cert = Cert::from_der(der);
    /// # }
    /// ```
    pub fn from_der(der: &[u8]) -> Result<Self, HttpClientError> {
        Ok(Self(X509::from_der(der).map_err(|e| {
            HttpClientError::from_error(ErrorKind::Build, e)
        })?))
    }

    /// Deserializes a list of PEM-formatted certificates.
    pub fn stack_from_pem(pem: &[u8]) -> Result<Vec<Self>, HttpClientError> {
        Ok(X509::stack_from_pem(pem)
            .map_err(|e| HttpClientError::from_error(ErrorKind::Build, e))?
            .into_iter()
            .map(Self)
            .collect())
    }
}

/// Represents a server X509 certificates.
///
/// You can use `from_pem` to parse a `&[u8]` into a list of certificates.
///
/// # Examples
///
/// ```
/// use ylong_http_client::Certificate;
///
/// fn from_pem(pem: &[u8]) {
///     let certs = Certificate::from_pem(pem);
/// }
/// ```
#[derive(Clone)]
pub struct Certificate {
    inner: CertificateList,
}

#[derive(Clone)]
pub(crate) enum CertificateList {
    CertList(Vec<Cert>),
    #[cfg(feature = "c_openssl_3_0")]
    PathList(String),
}

impl Certificate {
    /// Deserializes a list of PEM-formatted certificates.
    pub fn from_pem(pem: &[u8]) -> Result<Self, HttpClientError> {
        let cert_list = X509::stack_from_pem(pem)
            .map_err(|e| HttpClientError::from_error(ErrorKind::Build, e))?
            .into_iter()
            .map(Cert)
            .collect();
        Ok(Certificate {
            inner: CertificateList::CertList(cert_list),
        })
    }

    /// Deserializes a list of PEM-formatted certificates.
    #[cfg(feature = "c_openssl_3_0")]
    pub fn from_path(path: &str) -> Result<Self, HttpClientError> {
        Ok(Certificate {
            inner: CertificateList::PathList(path.to_string()),
        })
    }

    pub(crate) fn into_inner(self) -> CertificateList {
        self.inner
    }
}

#[cfg(test)]
mod ut_openssl_adapter {
    use crate::util::c_openssl::adapter::CertificateList;
    use crate::util::{Cert, TlsConfigBuilder, TlsFileType, TlsVersion};
    use crate::{AlpnProtocol, AlpnProtocolList, Certificate};

    /// UT test cases for `TlsConfigBuilder::new`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_tls_config_builder_new() {
        let _ = TlsConfigBuilder::default();
        let builder = TlsConfigBuilder::new();
        assert!(builder.ca_file("folder/ca.crt").build().is_err());
    }

    /// UT test cases for `TlsConfigBuilder::new`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_cipher_suites`.
    /// 3. Provides an invalid path as argument.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_set_cipher_suites() {
        let builder = TlsConfigBuilder::new().cipher_suites("INVALID STRING");
        assert!(builder.build().is_err());
    }

    /// UT test cases for `TlsConfigBuilder::set_max_proto_version`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_max_proto_version`.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_set_max_proto_version() {
        let builder = TlsConfigBuilder::new()
            .max_proto_version(TlsVersion::TLS_1_2)
            .build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `TlsConfigBuilder::set_min_proto_version`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_min_proto_version`.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_set_min_proto_version() {
        let builder = TlsConfigBuilder::new()
            .min_proto_version(TlsVersion::TLS_1_2)
            .build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `TlsConfigBuilder::set_cipher_list`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_cipher_list`.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_set_cipher_list() {
        let builder = TlsConfigBuilder::new()
            .cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
            .build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `TlsConfigBuilder::set_certificate_file`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_certificate_file`.
    /// 3. Provides an invalid path as argument.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_set_certificate_file() {
        let builder = TlsConfigBuilder::new()
            .certificate_file("cert.pem", TlsFileType::PEM)
            .build();
        assert!(builder.is_err());
    }

    /// UT test cases for `TlsConfigBuilder::set_certificate_chain_file`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_certificate_chain_file`.
    /// 3. Provides an invalid path as argument.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_set_certificate_chain_file() {
        let builder = TlsConfigBuilder::new()
            .certificate_chain_file("cert.pem")
            .build();
        assert!(builder.is_err());
    }

    /// UT test cases for `TlsConfigBuilder::add_root_certificates`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `add_root_certificates`.
    /// 3. Provides PEM-formatted certificates.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_add_root_certificates() {
        let certificate = Certificate::from_pem(include_bytes!("../../../tests/file/root-ca.pem"))
            .expect("Sets certs error.");
        #[cfg(feature = "c_openssl_1_1")]
        let CertificateList::CertList(certs) = certificate.inner;
        #[cfg(feature = "c_openssl_3_0")]
        let certs = match certificate.inner {
            CertificateList::CertList(c) => c,
            CertificateList::PathList(_) => vec![],
        };

        let builder = TlsConfigBuilder::new().add_root_certificates(certs).build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `Certificate::clone`.
    ///
    /// # Brief
    /// 1. Creates a `Certificate` by calling `Certificate::from_pem`.
    /// 2. Creates another `Certificate` by calling `Certificate::clone`.
    /// 3. Checks if the result is as expected.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn ut_certificate_clone() {
        let pem = include_bytes!("../../../tests/file/root-ca.pem");
        let certificate = Certificate::from_pem(pem).unwrap();
        drop(certificate.clone());
    }

    /// UT test cases for `Cert::clone`.
    ///
    /// # Brief
    /// 1. Creates a `Cert` by calling `Cert::from_pem`.
    /// 2. Creates another `Cert` by calling `Cert::clone`.
    /// 3. Checks if the result is as expected.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn ut_cert_clone() {
        let pem = include_bytes!("../../../tests/file/root-ca.pem");
        let cert = Cert::from_pem(pem).unwrap();
        drop(cert.clone());
    }

    /// UT test cases for `TlsConfigBuilder::build_in_root_certs`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `build_in_root_certs`.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_build_in_root_certs() {
        let builder = TlsConfigBuilder::new().build_in_root_certs(true).build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `TlsConfigBuilder::set_alpn_proto_list`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`.
    /// 2. Calls `set_alpn_proto_list`.
    /// 3. Provides `AlpnProtocol`s.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_set_alpn_proto_list() {
        let builder = TlsConfigBuilder::new()
            .alpn_proto_list(
                AlpnProtocolList::new()
                    .extend(AlpnProtocol::HTTP11)
                    .extend(AlpnProtocol::H2),
            )
            .build();
        assert!(builder.is_ok());
    }

    /// UT test cases for `TlsConfig::ssl`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfig` by calling `TlsConfigBuilder::new` and
    ///    `TlsConfigBuilder::build`.
    /// 2. Creates a `TlsSsl` by calling `TlsConfig::ssl_new`.
    /// 3. Calls `TlsSsl::into_inner`.
    /// 4. Checks if the result is as expected.
    #[test]
    fn ut_tls_ssl() {
        let config = TlsConfigBuilder::new()
            .build()
            .expect("TlsConfig build error.");
        let _ssl = config
            .ssl_new("host name")
            .expect("Ssl build error.")
            .into_inner();
    }

    /// UT test cases for `TlsConfig::ssl` and `SslRef::set_verify_hostname`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfig` by calling `TlsConfigBuilder::new` and
    ///    `TlsConfigBuilder::build`.
    /// 2. Sets hostname "" and verify_hostname.
    /// 3. Creates a `Ssl` by calling `TlsConfig::ssl_new` then creates a
    ///    `SslStream`.
    /// 4. Calls `write` and `read` by `SslStream`.
    /// 5. Checks if retures the segmentation fault `invalid memory reference`.
    #[cfg(feature = "sync")]
    #[test]
    fn ut_tls_ssl_verify_hostname() {
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let config = TlsConfigBuilder::new()
            .sni(false)
            .danger_accept_invalid_hostnames(false)
            .build()
            .expect("TlsConfig build error.");

        let domain = String::from("");
        let ssl = config
            .ssl_new(domain.as_str())
            .expect("Ssl build error.")
            .into_inner();
        let stream = TcpStream::connect("huawei.com:443").expect("Tcp stream error.");
        let mut tls_stream = ssl.connect(stream).expect("Tls stream error.");

        tls_stream
            .write_all(b"GET / HTTP/1.0\r\n\r\n")
            .expect("Stream write error.");
        let mut res = vec![];
        tls_stream
            .read_to_end(&mut res)
            .expect("Stream read error.");
        println!("{}", String::from_utf8_lossy(&res));
    }

    /// UT test cases for `Cert::from_pem`.
    ///
    /// # Brief
    /// 1. Creates a `Cert` by calling `Cert::from_pem`.
    /// 2. Provides an invalid pem as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_x509_from_pem() {
        let pem = "(pem-content)";
        let x509 = Cert::from_pem(pem.as_bytes());
        assert!(x509.is_err());

        let cert = include_bytes!("../../../tests/file/root-ca.pem");
        println!("{:?}", std::str::from_utf8(cert).unwrap());
        let x509 = Cert::from_pem(cert);
        assert!(x509.is_ok());
    }

    /// UT test cases for `Cert::from_der`.
    ///
    /// # Brief
    /// 1. Creates a `Cert` by calling `Cert::from_der`.
    /// 2. Provides an invalid der as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_x509_from_der() {
        let der = "(dar-content)";
        let x509 = Cert::from_der(der.as_bytes());
        assert!(x509.is_err());
    }

    /// UT test cases for `Cert::stack_from_pem`.
    ///
    /// # Brief
    /// 1. Creates a `Cert` by calling `Cert::stack_from_pem`.
    /// 2. Provides pem bytes as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_cert_stack_from_der() {
        let v = include_bytes!("../../../tests/file/root-ca.pem");
        let x509 = Cert::stack_from_pem(v);
        assert!(x509.is_ok());
    }

    /// UT test cases for `Certificate::from_pem`.
    ///
    /// # Brief
    /// 1. Creates a `Certificate` by calling `Certificate::from_pem`.
    /// 2. Provides pem bytes as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_certificate_from_pem() {
        let v = include_bytes!("../../../tests/file/root-ca.pem");
        let certs = Certificate::from_pem(v);
        assert!(certs.is_ok());
    }
}
