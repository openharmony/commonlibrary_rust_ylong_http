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

use std::mem::forget;

use libc::c_int;

use crate::util::c_openssl::x509::{X509StoreContextRef, X509};
use crate::{ErrorKind, HttpClientError};
/// ServerCerts is provided to fetch info from X509
pub struct ServerCerts<'a> {
    inner: &'a X509StoreContextRef,
}

impl<'a> ServerCerts<'a> {
    pub(crate) fn new(inner: &'a X509StoreContextRef) -> Self {
        Self { inner }
    }

    /// Gets cers version.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::ServerCerts;
    ///
    /// # fn cert_version(certs: &ServerCerts) {
    /// let version = certs.version();
    /// # }
    /// ```
    pub fn version(&self) -> Result<usize, HttpClientError> {
        let cert = self
            .inner
            .get_current_cert()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        Ok(cert.get_cert_version() as usize)
    }

    /// Gets certs name.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::ServerCerts;
    ///
    /// # fn cert_name(certs: &ServerCerts) {
    /// let name = certs.cert_name().unwrap();
    /// # }
    /// ```
    pub fn cert_name(&self) -> Result<String, HttpClientError> {
        let cert = self
            .inner
            .get_current_cert()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let cert_name = cert
            .get_cert_name()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let mut buf = [0u8; 128];
        let size = 128;
        let res = cert_name.get_x509_name_info(buf.as_mut(), size as c_int);
        forget(cert_name);
        Ok(res)
    }

    /// Gets certs issuer.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::ServerCerts;
    ///
    /// # fn cert_issuer(certs: &ServerCerts) {
    /// let issuer = certs.issuer().unwrap();
    /// # }
    /// ```
    pub fn issuer(&self) -> Result<String, HttpClientError> {
        let cert = self
            .inner
            .get_current_cert()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let cert_issuer = cert
            .get_issuer_name()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let mut buf = [0u8; 128];
        let size = 128;
        let res = cert_issuer.get_x509_name_info(buf.as_mut(), size as c_int);
        forget(cert_issuer);
        Ok(res)
    }

    /// Compares certs, if they are same, return 1, if they are different,
    /// return 0.
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::ServerCerts;
    /// # use std::io::Read;
    /// # fn cmp_certs(certs: &ServerCerts) {
    /// # let mut file = std::fs::File::open("./examples/cert/cert.pem").unwrap();
    /// # let mut contents = String::new();
    /// # file.read_to_string(&mut contents).unwrap();
    /// let res = certs.cmp_pem_cert(contents.as_bytes()).unwrap();
    /// # }
    /// ```
    pub fn cmp_pem_cert(&self, target_pem: &[u8]) -> Result<usize, HttpClientError> {
        let cert = self
            .inner
            .get_current_cert()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let cert_key = cert
            .get_cert()
            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
        let target_cert = X509::from_pem(target_pem)
            .map_err(|e| HttpClientError::from_error(ErrorKind::Build, e))?;
        Ok(target_cert.cmp_certs(cert_key) as usize)
    }
}

impl AsRef<X509StoreContextRef> for ServerCerts<'_> {
    fn as_ref(&self) -> &X509StoreContextRef {
        self.inner
    }
}
