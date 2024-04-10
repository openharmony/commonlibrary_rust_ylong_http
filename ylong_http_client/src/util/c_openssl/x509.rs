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

use core::{ffi, fmt, ptr, str};
#[cfg(feature = "c_openssl_3_0")]
use std::ffi::CString;
use std::net::IpAddr;

use libc::{c_int, c_long, c_uint};

use super::bio::BioSlice;
use super::error::{error_get_lib, error_get_reason, ErrorStack};
use super::ffi::err::{ERR_clear_error, ERR_peek_last_error};
use super::ffi::pem::PEM_read_bio_X509;
#[cfg(feature = "c_openssl_3_0")]
use super::ffi::x509::X509_STORE_load_path;
use super::ffi::x509::{
    d2i_X509, EVP_PKEY_free, X509_NAME_free, X509_NAME_oneline, X509_PUBKEY_free,
    X509_STORE_CTX_free, X509_STORE_CTX_get0_cert, X509_STORE_add_cert, X509_STORE_free,
    X509_STORE_new, X509_VERIFY_PARAM_free, X509_VERIFY_PARAM_set1_host, X509_VERIFY_PARAM_set1_ip,
    X509_VERIFY_PARAM_set_hostflags, X509_get_issuer_name, X509_get_pubkey, X509_get_subject_name,
    X509_get_version, X509_up_ref, X509_verify, X509_verify_cert_error_string, EVP_PKEY,
    STACK_X509, X509_NAME, X509_PUBKEY, X509_STORE, X509_STORE_CTX, X509_VERIFY_PARAM,
};
use super::foreign::{Foreign, ForeignRef};
use super::stack::Stackof;
use super::{check_ptr, check_ret, ssl_init};
use crate::util::c_openssl::ffi::x509::{X509_free, C_X509};

foreign_type!(
    type CStruct = C_X509;
    fn drop = X509_free;
    pub(crate) struct X509;
    pub(crate) struct X509Ref;
);

foreign_type!(
    type CStruct = X509_NAME;
    fn drop = X509_NAME_free;
    pub(crate) struct X509Name;
    pub(crate) struct X509NameRef;
);

foreign_type! {
    type CStruct = EVP_PKEY;
    fn drop = EVP_PKEY_free;
    pub(crate) struct EvpPkey;
    pub(crate) struct EvpPkeyRef;
}

const ERR_LIB_PEM: c_int = 9;
const PEM_R_NO_START_LINE: c_int = 108;

impl X509 {
    pub(crate) fn from_pem(pem: &[u8]) -> Result<X509, ErrorStack> {
        ssl_init();
        let bio = BioSlice::from_byte(pem)?;
        let ptr = check_ptr(unsafe {
            PEM_read_bio_X509(bio.as_ptr(), ptr::null_mut(), None, ptr::null_mut())
        })?;
        Ok(X509::from_ptr(ptr))
    }

    pub(crate) fn from_der(der: &[u8]) -> Result<X509, ErrorStack> {
        ssl_init();
        let len =
            ::std::cmp::min(der.len(), ::libc::c_long::max_value() as usize) as ::libc::c_long;
        let ptr = check_ptr(unsafe { d2i_X509(ptr::null_mut(), &mut der.as_ptr(), len) })?;
        Ok(X509::from_ptr(ptr))
    }

    /// Deserializes a list of PEM-formatted certificates.
    pub(crate) fn stack_from_pem(pem: &[u8]) -> Result<Vec<X509>, ErrorStack> {
        unsafe {
            ssl_init();
            let bio = BioSlice::from_byte(pem)?;

            let mut certs = vec![];
            loop {
                let r = PEM_read_bio_X509(bio.as_ptr(), ptr::null_mut(), None, ptr::null_mut());
                if r.is_null() {
                    let err = ERR_peek_last_error();
                    if error_get_lib(err) == ERR_LIB_PEM
                        && error_get_reason(err) == PEM_R_NO_START_LINE
                    {
                        ERR_clear_error();
                        break;
                    }

                    return Err(ErrorStack::get());
                } else {
                    certs.push(X509(r));
                }
            }
            Ok(certs)
        }
    }
}

impl X509Ref {
    pub(crate) fn get_cert_version(&self) -> c_long {
        unsafe { X509_get_version(self.as_ptr() as *const _) }
    }

    pub(crate) fn get_cert_name(&self) -> Result<X509Name, ErrorStack> {
        Ok(X509Name(check_ptr(unsafe {
            X509_get_subject_name(self.as_ptr() as *const _)
        })?))
    }

    pub(crate) fn get_issuer_name(&self) -> Result<X509Name, ErrorStack> {
        Ok(X509Name(check_ptr(unsafe {
            X509_get_issuer_name(self.as_ptr() as *const _)
        })?))
    }

    pub(crate) fn get_cert(&self) -> Result<EvpPkey, ErrorStack> {
        Ok(EvpPkey(check_ptr(unsafe {
            X509_get_pubkey(self.as_ptr() as *mut _)
        })?))
    }

    pub(crate) fn cmp_certs(&self, pkey: EvpPkey) -> c_int {
        unsafe { X509_verify(self.as_ptr() as *mut _, pkey.as_ptr()) }
    }
}

impl X509Name {
    pub(crate) fn get_x509_name_info(&self, buf: &mut [u8], size: c_int) -> String {
        unsafe {
            let _ = X509_NAME_oneline(self.as_ptr() as *mut _, buf.as_mut_ptr() as *mut _, size);
            let res = str::from_utf8(buf).unwrap_or("").to_string();
            res
        }
    }
}
impl Stackof for X509 {
    type StackType = STACK_X509;
}

impl Clone for X509 {
    fn clone(&self) -> Self {
        X509Ref::to_owned(self)
    }
}

impl ToOwned for X509Ref {
    type Owned = X509;

    fn to_owned(&self) -> Self::Owned {
        unsafe {
            X509_up_ref(self.as_ptr());
            X509::from_ptr(self.as_ptr())
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct X509VerifyResult(c_int);

impl X509VerifyResult {
    fn error_string(&self) -> &'static str {
        ssl_init();
        unsafe {
            let s = X509_verify_cert_error_string(self.0 as c_long);
            str::from_utf8(ffi::CStr::from_ptr(s).to_bytes()).unwrap_or("")
        }
    }

    pub(crate) fn from_raw(err: c_int) -> X509VerifyResult {
        X509VerifyResult(err)
    }
}

impl fmt::Display for X509VerifyResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(self.error_string())
    }
}

#[cfg(test)]
impl fmt::Debug for X509VerifyResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("X509VerifyResult")
            .field("code", &self.0)
            .field("error", &self.error_string())
            .finish()
    }
}

foreign_type!(
    type CStruct = X509_STORE;
    fn drop = X509_STORE_free;
    pub(crate) struct X509Store;
    pub(crate) struct X509StoreRef;
);

impl X509Store {
    pub(crate) fn new() -> Result<X509Store, ErrorStack> {
        ssl_init();
        Ok(X509Store(check_ptr(unsafe { X509_STORE_new() })?))
    }
}

impl X509StoreRef {
    pub(crate) fn add_cert(&mut self, cert: X509) -> Result<(), ErrorStack> {
        check_ret(unsafe { X509_STORE_add_cert(self.as_ptr(), cert.as_ptr()) }).map(|_| ())
    }

    #[cfg(feature = "c_openssl_3_0")]
    pub(crate) fn add_path(&mut self, path: String) -> Result<(), ErrorStack> {
        let p_slice: &str = &path;
        let path = match CString::new(p_slice) {
            Ok(cstr) => cstr,
            Err(_) => return Err(ErrorStack::get()),
        };
        check_ret(unsafe { X509_STORE_load_path(self.as_ptr(), path.as_ptr() as *const _) })
            .map(|_| ())
    }
}

foreign_type!(
    type CStruct = X509_VERIFY_PARAM;
    fn drop = X509_VERIFY_PARAM_free;
    pub(crate) struct X509VerifyParam;
    pub(crate) struct X509VerifyParamRef;
);

pub(crate) const X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS: c_uint = 0x4;

impl X509VerifyParamRef {
    pub(crate) fn set_hostflags(&mut self, hostflags: c_uint) {
        unsafe {
            X509_VERIFY_PARAM_set_hostflags(self.as_ptr(), hostflags);
        }
    }

    pub(crate) fn set_host(&mut self, host: &str) -> Result<(), ErrorStack> {
        let c_host = if host.is_empty() { "\0" } else { host };
        check_ret(unsafe {
            // Must ensure name is NUL-terminated when namelen == 0.
            X509_VERIFY_PARAM_set1_host(self.as_ptr(), c_host.as_ptr() as *const _, host.len())
        })
        .map(|_| ())
    }

    pub(crate) fn set_ip(&mut self, ip_addr: IpAddr) -> Result<(), ErrorStack> {
        let mut v = [0u8; 16];
        let len = match ip_addr {
            IpAddr::V4(addr) => {
                v[..4].copy_from_slice(&addr.octets());
                4
            }
            IpAddr::V6(addr) => {
                v.copy_from_slice(&addr.octets());
                16
            }
        };
        check_ret(unsafe { X509_VERIFY_PARAM_set1_ip(self.as_ptr(), v.as_ptr() as *const _, len) })
            .map(|_| ())
    }
}

foreign_type! {
    type CStruct = X509_STORE_CTX;
    fn drop = X509_STORE_CTX_free;
    pub(crate) struct X509StoreContext;
    pub(crate) struct X509StoreContextRef;
}

impl X509StoreContextRef {
    pub(crate) fn get_current_cert(&self) -> Result<&X509Ref, ErrorStack> {
        unsafe {
            Ok(X509Ref::from_ptr(check_ptr(X509_STORE_CTX_get0_cert(
                self.as_ptr() as *const _,
            ))?))
        }
    }
}

foreign_type!(
    type CStruct = X509_PUBKEY;
    fn drop = X509_PUBKEY_free;
    pub(crate) struct X509PubKey;
    pub(crate) struct X509PubKeyRef;
);

#[cfg(test)]
mod ut_x509 {

    /// UT test cases for `X509::clone`.
    ///
    /// # Brief
    /// 1. Creates a `X509` by calling `X509::from_pem`.
    /// 2. Creates another `X509` by calling `X509::clone`.
    /// 3. Checks if the result is as expected.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn ut_x509_clone() {
        use crate::util::c_openssl::x509::X509;

        let pem = include_bytes!("../../../tests/file/root-ca.pem");
        let x509 = X509::from_pem(pem).unwrap();
        drop(x509.clone());
    }
}
