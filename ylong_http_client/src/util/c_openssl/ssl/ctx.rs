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

use super::{filetype::SslFiletype, method::SslMethod, version::SslVersion};
use crate::{
    c_openssl::{
        ffi::ssl::{
            SSL_CTX_free, SSL_CTX_get_cert_store, SSL_CTX_set_default_verify_paths,
            SSL_CTX_set_verify,
        },
        x509::{X509Store, X509StoreRef},
    },
    util::c_openssl::{
        check_ptr, check_ret,
        error::ErrorStack,
        ffi::ssl::{
            SSL_CTX_ctrl, SSL_CTX_load_verify_locations, SSL_CTX_new, SSL_CTX_set_alpn_protos,
            SSL_CTX_set_cert_store, SSL_CTX_set_cipher_list, SSL_CTX_set_ciphersuites,
            SSL_CTX_up_ref, SSL_CTX_use_certificate, SSL_CTX_use_certificate_chain_file,
            SSL_CTX_use_certificate_file, SSL_CTX,
        },
        foreign::{Foreign, ForeignRef},
        ssl_init,
        x509::{X509Ref, X509},
    },
};
use core::{fmt, mem, ptr};
use libc::{c_int, c_long, c_uint, c_void};
use std::{ffi::CString, path::Path};

const SSL_CTRL_EXTRA_CHAIN_CERT: c_int = 14;

const SSL_CTRL_SET_MIN_PROTO_VERSION: c_int = 123;
const SSL_CTRL_SET_MAX_PROTO_VERSION: c_int = 124;

foreign_type!(
    type CStruct = SSL_CTX;
    fn drop = SSL_CTX_free;
    pub(crate) struct SslContext;
    pub(crate) struct SslContextRef;
);

impl SslContext {
    pub(crate) fn builder(method: SslMethod) -> SslContextBuilder {
        SslContextBuilder::new(method)
    }
}

// TODO: add useful info here.
impl fmt::Debug for SslContext {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "SslContext")
    }
}

impl Clone for SslContext {
    fn clone(&self) -> Self {
        (**self).to_owned()
    }
}

impl ToOwned for SslContextRef {
    type Owned = SslContext;

    fn to_owned(&self) -> Self::Owned {
        unsafe {
            SSL_CTX_up_ref(self.as_ptr());
            SslContext::from_ptr(self.as_ptr())
        }
    }
}

const SSL_VERIFY_PEER: c_int = 1;

/// A builder for `SslContext`.
pub(crate) struct SslContextBuilder(Result<SslContext, ErrorStack>);

impl SslContextBuilder {
    pub(crate) fn new(method: SslMethod) -> Self {
        ssl_init();
        let ptr = match check_ptr(unsafe { SSL_CTX_new(method.as_ptr()) }) {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };
        if let Err(e) = check_ret(unsafe { SSL_CTX_set_default_verify_paths(ptr) }) {
            return SslContextBuilder(Err(e));
        };

        unsafe {
            SSL_CTX_set_verify(ptr, SSL_VERIFY_PEER, None);
        }

        SslContextBuilder::from_ptr(ptr).set_cipher_list(
            "DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK",
        )
    }

    /// Creates a `SslContextBuilder` from a `SSL_CTX`.
    pub(crate) fn from_ptr(ptr: *mut SSL_CTX) -> Self {
        SslContextBuilder(Ok(SslContext(ptr)))
    }

    /// Creates a `SslContextBuilder` from a `SSL_CTX`.
    pub(crate) fn as_ptr(&self) -> Result<*mut SSL_CTX, ErrorStack> {
        match &self.0 {
            Ok(ctx) => Ok(ctx.0),
            Err(e) => Err(e.to_owned()),
        }
    }

    /// Builds a `SslContext`.
    pub(crate) fn build(self) -> Result<SslContext, ErrorStack> {
        self.0
    }

    pub(crate) fn from_error(e: ErrorStack) -> Self {
        SslContextBuilder(Err(e))
    }

    pub(crate) fn set_min_proto_version(self, version: SslVersion) -> Self {
        let ptr = match self.as_ptr() {
            Ok(p) => p,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_ctrl(
                ptr,
                SSL_CTRL_SET_MIN_PROTO_VERSION,
                version.0 as c_long,
                ptr::null_mut(),
            )
        } as c_int)
        {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    pub(crate) fn set_max_proto_version(self, version: SslVersion) -> Self {
        let ptr = match self.as_ptr() {
            Ok(p) => p,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_ctrl(
                ptr,
                SSL_CTRL_SET_MAX_PROTO_VERSION,
                version.0 as c_long,
                ptr::null_mut(),
            )
        } as c_int)
        {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Loads trusted root certificates from a file.\
    /// Uses to Set default locations for trusted CA certificates.
    ///
    /// The file should contain a sequence of PEM-formatted CA certificates.
    pub(crate) fn set_ca_file<P>(self, file: P) -> Self
    where
        P: AsRef<Path>,
    {
        let path = match file.as_ref().as_os_str().to_str() {
            Some(path) => path,
            None => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let file = match CString::new(path) {
            Ok(path) => path,
            Err(_) => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_load_verify_locations(ptr, file.as_ptr() as *const _, ptr::null())
        }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Sets the list of supported ciphers for protocols before `TLSv1.3`.
    pub(crate) fn set_cipher_list(self, list: &str) -> Self {
        let list = match CString::new(list) {
            Ok(cstr) => cstr,
            Err(_) => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe { SSL_CTX_set_cipher_list(ptr, list.as_ptr() as *const _) }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Sets the list of supported ciphers for the `TLSv1.3` protocol.
    pub(crate) fn set_cipher_suites(self, list: &str) -> Self {
        let list = match CString::new(list) {
            Ok(cstr) => cstr,
            Err(_) => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe { SSL_CTX_set_ciphersuites(ptr, list.as_ptr() as *const _) }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Loads a leaf certificate from a file.
    ///
    /// Only a single certificate will be loaded - use `add_extra_chain_cert` to add the remainder
    /// of the certificate chain, or `set_certificate_chain_file` to load the entire chain from a
    /// single file.
    pub(crate) fn set_certificate_file<P>(self, file: P, file_type: SslFiletype) -> Self
    where
        P: AsRef<Path>,
    {
        let path = match file.as_ref().as_os_str().to_str() {
            Some(path) => path,
            None => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let file = match CString::new(path) {
            Ok(path) => path,
            Err(_) => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_use_certificate_file(ptr, file.as_ptr() as *const _, file_type.as_raw())
        }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Loads a certificate chain from file into ctx.
    /// The certificates must be in PEM format and must be sorted starting with
    /// the subject's certificate (actual client or server certificate), followed
    /// by intermediate CA certificates if applicable, and ending at the highest
    /// level (root) CA.
    pub(crate) fn set_certificate_chain_file<P>(self, file: P) -> Self
    where
        P: AsRef<Path>,
    {
        let path = match file.as_ref().as_os_str().to_str() {
            Some(path) => path,
            None => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let file = match CString::new(path) {
            Ok(path) => path,
            Err(_) => return SslContextBuilder(Err(ErrorStack::get())),
        };
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_use_certificate_chain_file(ptr, file.as_ptr() as *const _)
        }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Sets the leaf certificate.
    ///
    /// Use `add_extra_chain_cert` to add the remainder of the certificate chain.
    pub(crate) fn set_certificate(self, key: &X509Ref) -> Self {
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe { SSL_CTX_use_certificate(ptr, key.as_ptr()) }) {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Appends a certificate to the certificate chain.
    ///
    /// This chain should contain all certificates necessary to go from the certificate specified by
    /// `set_certificate` to a trusted root.
    pub(crate) fn add_extra_chain_cert(self, cert: X509) -> Self {
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match check_ret(unsafe {
            SSL_CTX_ctrl(
                ptr,
                SSL_CTRL_EXTRA_CHAIN_CERT,
                0,
                cert.as_ptr() as *mut c_void,
            )
        } as c_int)
        {
            Ok(_num) => self,
            Err(e) => SslContextBuilder(Err(e)),
        }
    }

    /// Sets the protocols to sent to the server for Application Layer Protocol Negotiation (ALPN).
    pub(crate) fn set_alpn_protos(self, protocols: &[u8]) -> Self {
        assert!(protocols.len() <= c_uint::max_value() as usize);

        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };

        match unsafe { SSL_CTX_set_alpn_protos(ptr, protocols.as_ptr(), protocols.len() as c_uint) }
        {
            0 => self,
            _ => SslContextBuilder(Err(ErrorStack::get())),
        }
    }

    pub(crate) fn set_cert_store(self, cert_store: X509Store) -> Self {
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return SslContextBuilder(Err(e)),
        };
        unsafe {
            SSL_CTX_set_cert_store(ptr, cert_store.as_ptr());
            mem::forget(cert_store);
        }
        self
    }

    pub(crate) fn cert_store_mut(&mut self) -> Result<&mut X509StoreRef, ErrorStack> {
        let ptr = match self.as_ptr() {
            Ok(ptr) => ptr,
            Err(e) => return Err(e),
        };
        Ok(unsafe { X509StoreRef::from_ptr_mut(SSL_CTX_get_cert_store(ptr)) })
    }
}
