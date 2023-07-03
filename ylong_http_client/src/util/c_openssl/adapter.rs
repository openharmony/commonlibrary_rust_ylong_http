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

use crate::{error::HttpClientError, util::AlpnProtocolList};
use crate::{
    util::c_openssl::{
        error::ErrorStack,
        ssl::{Ssl, SslContext, SslContextBuilder, SslFiletype, SslMethod, SslVersion},
        x509::{X509Ref, X509Store, X509},
    },
    ErrorKind,
};
use std::{net::IpAddr, path::Path};

/// `TlsContextBuilder` implementation based on `SSL_CTX`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::util::{TlsConfigBuilder, TlsVersion};
///
/// let context = TlsConfigBuilder::new()
///     .set_ca_file("ca.crt")
///     .set_max_proto_version(TlsVersion::TLS_1_2)
///     .set_min_proto_version(TlsVersion::TLS_1_2)
///     .set_cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
///     .build();
/// ```
pub struct TlsConfigBuilder {
    inner: SslContextBuilder,
}

impl TlsConfigBuilder {
    /// Creates a new, default `SslContextBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            inner: SslContext::builder(SslMethod::tls_client()),
        }
    }

    /// Loads trusted root certificates from a file. The file should contain a
    /// sequence of PEM-formatted CA certificates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_ca_file("ca.crt");
    /// ```
    pub fn set_ca_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.inner = self.inner.set_ca_file(path);
        self
    }

    /// Sets the maximum supported protocol version. A value of `None` will
    /// enable protocol versions down the the highest version supported by `OpenSSL`.
    ///
    /// Requires `OpenSSL 1.1.0` or or `LibreSSL 2.6.1` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::{TlsConfigBuilder, TlsVersion};
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_max_proto_version(TlsVersion::TLS_1_2);
    /// ```
    pub fn set_max_proto_version(mut self, version: TlsVersion) -> Self {
        self.inner = self.inner.set_max_proto_version(version.into_inner());
        self
    }

    /// Sets the minimum supported protocol version. A value of `None` will
    /// enable protocol versions down the the lowest version supported by `OpenSSL`.
    ///
    /// Requires `OpenSSL 1.1.0` or `LibreSSL 2.6.1` or newer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::{TlsConfigBuilder, TlsVersion};
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_min_proto_version(TlsVersion::TLS_1_2);
    /// ```
    pub fn set_min_proto_version(mut self, version: TlsVersion) -> Self {
        self.inner = self.inner.set_min_proto_version(version.into_inner());
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
    /// use ylong_http_client::util::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_cipher_list(
    ///         "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK"
    ///     );
    /// ```
    pub fn set_cipher_list(mut self, list: &str) -> Self {
        self.inner = self.inner.set_cipher_list(list);
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
    /// use ylong_http_client::util::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_cipher_suites(
    ///         "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK"
    ///     );
    /// ```
    pub fn set_cipher_suites(mut self, list: &str) -> Self {
        self.inner = self.inner.set_cipher_suites(list);
        self
    }

    /// Loads a leaf certificate from a file.
    ///
    /// Only a single certificate will be loaded - use `add_extra_chain_cert` to
    /// add the remainder of the certificate chain, or `set_certificate_chain_file`
    /// to load the entire chain from a single file.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::{TlsConfigBuilder, TlsFileType};
    ///
    /// let builder = TlsConfigBuilder::new()
    ///     .set_certificate_file("cert.pem", TlsFileType::PEM);
    /// ```
    pub fn set_certificate_file<T: AsRef<Path>>(mut self, path: T, file_type: TlsFileType) -> Self {
        self.inner = self
            .inner
            .set_certificate_file(path, file_type.into_inner());
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
    /// use ylong_http_client::util::TlsConfigBuilder;
    ///
    /// let builder = TlsConfigBuilder::new().set_certificate_chain_file("cert.pem");
    /// ```
    pub fn set_certificate_chain_file<T: AsRef<Path>>(mut self, path: T) -> Self {
        self.inner = self.inner.set_certificate_chain_file(path);
        self
    }

    /// Sets the leaf certificate.
    ///
    /// Use `add_extra_chain_cert` to add the remainder of the certificate chain.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_http_client::util::{TlsConfigBuilder, Cert};
    ///
    /// let x509 = Cert::from_pem(b"pem-content").unwrap();
    /// let builder = TlsConfigBuilder::new().set_certificate(&x509);
    /// ```
    pub fn set_certificate(mut self, cert: &Cert) -> Self {
        self.inner = self.inner.set_certificate(cert.as_ref());
        self
    }

    /// Appends a certificate to the certificate chain.
    ///
    /// This chain should contain all certificates necessary to go from the
    /// certificate specified by `set_certificate` to a trusted root.
    ///
    /// This method is based on `openssl::SslContextBuilder::add_extra_chain_cert`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_http_client::util::{TlsConfigBuilder, Cert};
    ///
    /// let root = Cert::from_pem(b"pem-content").unwrap();
    /// let chain = Cert::from_pem(b"pem-content").unwrap();
    /// let builder = TlsConfigBuilder::new()
    ///     .set_certificate(&root)
    ///     .add_extra_chain_cert(chain);
    /// ```
    pub fn add_extra_chain_cert(mut self, cert: Cert) -> Self {
        self.inner = self.inner.add_extra_chain_cert(cert.into_inner());
        self
    }

    /// Adds custom root certificate.
    pub fn add_root_certificates(mut self, certs: Certificate) -> Self {
        for cert in certs.inner {
            let store = match self.inner.cert_store_mut() {
                Ok(store) => store,
                Err(e) => {
                    self.inner = SslContextBuilder::from_error(e);
                    return self;
                }
            };
            if let Err(e) = store.add_cert(cert.0) {
                self.inner = SslContextBuilder::from_error(e);
                return self;
            }
        }

        self
    }

    /// Sets the protocols to sent to the server for Application Layer Protocol Negotiation (ALPN).
    ///
    /// Requires OpenSSL 1.0.2 or LibreSSL 2.6.1 or newer.
    ///
    /// # Examples
    /// ```
    /// use ylong_http_client::util::{TlsConfigBuilder};
    ///
    /// let protocols = b"\x06spdy/1\x08http/1.1";
    /// let builder = TlsConfigBuilder::new().set_alpn_protos(protocols);
    /// ```
    pub fn set_alpn_protos(mut self, protocols: &[u8]) -> Self {
        self.inner = self.inner.set_alpn_protos(protocols);
        self
    }

    /// Sets the protocols to sent to the server for Application Layer Protocol Negotiation (ALPN).
    ///
    /// This method is based on `openssl::SslContextBuilder::set_alpn_protos`.
    ///
    /// Requires OpenSSL 1.0.2 or LibreSSL 2.6.1 or newer.
    ///
    /// # Examples
    /// ```
    /// use ylong_http_client::util::{AlpnProtocol, AlpnProtocolList, TlsConfigBuilder};
    ///
    /// let protocols = AlpnProtocolList::new()
    ///     .extend(AlpnProtocol::SPDY1)
    ///     .extend(AlpnProtocol::HTTP11);
    /// let builder = TlsConfigBuilder::new().set_alpn_protos(protocols.as_slice());
    /// ```
    pub fn set_alpn_proto_list(mut self, list: AlpnProtocolList) -> Self {
        self.inner = self.inner.set_alpn_protos(list.as_slice());
        self
    }

    /// Controls the use of built-in system certificates during certificate validation.
    /// Default to `true` -- uses built-in system certs.
    pub fn build_in_root_certs(mut self, is_use: bool) -> Self {
        if !is_use {
            let cert_store = X509Store::new();
            match cert_store {
                Ok(store) => self.inner = self.inner.set_cert_store(store),
                Err(e) => self.inner = SslContextBuilder::from_error(e),
            }
        }
        self
    }

    /// Builds a `TlsContext`. Returns `Err` if an error occurred during configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::{TlsConfigBuilder, TlsVersion};
    ///
    /// let context = TlsConfigBuilder::new()
    ///     .set_ca_file("ca.crt")
    ///     .set_max_proto_version(TlsVersion::TLS_1_2)
    ///     .set_min_proto_version(TlsVersion::TLS_1_2)
    ///     .set_cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
    ///     .build();
    /// ```
    pub fn build(self) -> Result<TlsConfig, HttpClientError> {
        Ok(TlsConfig(self.inner.build().map_err(|e| {
            HttpClientError::new_with_cause(ErrorKind::Build, Some(e))
        })?))
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
/// use ylong_http_client::util::TlsConfig;
///
/// let builder = TlsConfig::builder();
/// ```
#[derive(Debug, Clone)]
pub struct TlsConfig(SslContext);

impl TlsConfig {
    /// Creates a new, default `TlsContextBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::TlsConfig;
    ///
    /// let builder = TlsConfig::builder();
    /// ```
    pub fn builder() -> TlsConfigBuilder {
        TlsConfigBuilder::new()
    }

    /// Creates a new, default `TlsSsl`.
    pub(crate) fn ssl(&self) -> Result<TlsSsl, ErrorStack> {
        let ctx = &self.0;
        let ssl = Ssl::new(ctx)?;
        Ok(TlsSsl(ssl))
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        TlsConfig::builder().build().unwrap()
    }
}

/// /// `TlsSsl` is based on `Ssl`
pub(crate) struct TlsSsl(Ssl);

impl TlsSsl {
    pub(crate) fn into_inner(self) -> Ssl {
        self.0
    }

    pub(crate) fn set_sni_verify(&mut self, name: &str) -> Result<(), ErrorStack> {
        let ssl = &mut self.0;
        if name.parse::<IpAddr>().is_err() {
            ssl.set_host_name(name)?;
        }
        Ok(())
    }
}

/// `TlsVersion` is based on `openssl::SslVersion`, which provides `SSL/TLS`
/// protocol version.
///
/// # Examples
///
/// ```
/// use ylong_http_client::util::TlsVersion;
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
/// use ylong_http_client::util::TlsFileType;
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
            HttpClientError::new_with_cause(ErrorKind::Build, Some(e))
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
            HttpClientError::new_with_cause(ErrorKind::Build, Some(e))
        })?))
    }

    /// Deserializes a list of PEM-formatted certificates.
    pub fn stack_from_pem(pem: &[u8]) -> Result<Vec<Self>, HttpClientError> {
        Ok(X509::stack_from_pem(pem)
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Build, Some(e)))?
            .into_iter()
            .map(Self)
            .collect())
    }

    /// Gets a reference to `X509Ref`.
    pub(crate) fn as_ref(&self) -> &X509Ref {
        self.0.as_ref()
    }

    /// Consumes `X509` and then takes `X509`.
    pub(crate) fn into_inner(self) -> X509 {
        self.0
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
pub struct Certificate {
    inner: Vec<Cert>,
}

impl Certificate {
    /// Deserializes a list of PEM-formatted certificates.
    pub fn from_pem(pem: &[u8]) -> Result<Self, HttpClientError> {
        let inner = X509::stack_from_pem(pem)
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Build, Some(e)))?
            .into_iter()
            .map(Cert)
            .collect();
        Ok(Certificate { inner })
    }

    pub(crate) fn into_inner(self) -> Vec<Cert> {
        self.inner
    }
}

#[cfg(test)]
mod ut_openssl_adapter {
    use crate::util::{Cert, TlsConfigBuilder, TlsFileType, TlsVersion};

    /// UT test cases for `TlsConfigBuilder::new`.
    ///
    /// # Brief
    /// 1. Creates a `TlsConfigBuilder` by calling `TlsConfigBuilder::new`
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_tls_config_builder_new() {
        let _ = TlsConfigBuilder::default();
        let builder = TlsConfigBuilder::new();
        assert!(builder.set_ca_file("folder/ca.crt").build().is_err());
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
        let builder = TlsConfigBuilder::new().set_cipher_suites("INVALID STRING");
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
            .set_max_proto_version(TlsVersion::TLS_1_2)
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
            .set_min_proto_version(TlsVersion::TLS_1_2)
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
            .set_cipher_list(
                "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK",
            )
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
            .set_certificate_file("cert.pem", TlsFileType::PEM)
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
            .set_certificate_chain_file("cert.pem")
            .build();
        assert!(builder.is_err());
    }

    /// UT test cases for `X509::from_pem`.
    ///
    /// # Brief
    /// 1. Creates a `X509` by calling `X509::from_pem`.
    /// 2. Provides an invalid pem as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_x509_from_pem() {
        let pem = "(pem-content)";
        let x509 = Cert::from_pem(pem.as_bytes());
        // println!("{:?}", x509);
        assert!(x509.is_err());

        let cert = include_bytes!("../../../tests/file/root-ca.pem");
        println!("{:?}", std::str::from_utf8(cert).unwrap());
        // let debugged = format!("{:#?}", cert);
        // println!("{debugged}");
        let x509 = Cert::from_pem(cert);
        // println!("{:?}", x509);
        assert!(x509.is_ok());
    }

    /// UT test cases for `X509::from_der`.
    ///
    /// # Brief
    /// 1. Creates a `X509` by calling `X509::from_der`.
    /// 2. Provides an invalid der as argument.
    /// 3. Checks if the result is as expected.
    #[test]
    fn ut_x509_from_der() {
        let der = "(dar-content)";
        let x509 = Cert::from_der(der.as_bytes());
        assert!(x509.is_err());
    }
}
