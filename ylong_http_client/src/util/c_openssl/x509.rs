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
    check_ptr,
    error::{error_get_lib, error_get_reason, ErrorStack},
    ffi::{
        err::{ERR_clear_error, ERR_peek_last_error},
        pem::PEM_read_bio_X509,
        x509::{
            d2i_X509, X509_STORE_free, X509_STORE_new, X509_verify_cert_error_string, X509_STORE,
        },
    },
    foreign::Foreign,
    ssl_init,
};
use crate::util::c_openssl::ffi::x509::{X509_free, C_X509};
use core::{ffi, fmt, ptr, str};
use libc::{c_int, c_long};

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
        Ok(X509Store(check_ptr(unsafe { X509_STORE_new() })?))
    }
}
