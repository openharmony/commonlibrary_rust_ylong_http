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

use super::{
    bio::BioSlice,
    check_ptr, check_ret,
    error::{error_get_lib, error_get_reason, ErrorStack},
    ffi::{
        err::{ERR_clear_error, ERR_peek_last_error},
        pem::PEM_read_bio_X509,
        x509::{
            d2i_X509, X509_STORE_add_cert, X509_STORE_free, X509_STORE_new, X509_VERIFY_PARAM_free,
            X509_VERIFY_PARAM_set1_host, X509_VERIFY_PARAM_set1_ip,
            X509_VERIFY_PARAM_set_hostflags, X509_verify_cert_error_string, STACK_X509, X509_STORE,
            X509_VERIFY_PARAM,
        },
    },
    foreign::{Foreign, ForeignRef},
    ssl_init,
    stack::Stackof,
};
use crate::util::c_openssl::ffi::x509::{X509_free, C_X509};
use core::{ffi, fmt, ptr, str};
use libc::{c_int, c_long, c_uint};
use std::net::IpAddr;

foreign_type!(
    type CStruct = C_X509;
    fn drop = X509_free;
    pub(crate) struct X509;
    pub(crate) struct X509Ref;
);

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

impl Stackof for X509 {
    type StackType = STACK_X509;
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

impl fmt::Debug for X509VerifyResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("X509VerifyResult")
            .field("code", &self.0)
            .field("error", &self.error_string())
            .finish()
    }
}

impl fmt::Display for X509VerifyResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(self.error_string())
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
        check_ret(unsafe {
            X509_VERIFY_PARAM_set1_host(self.as_ptr(), host.as_ptr() as *const _, host.len())
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
