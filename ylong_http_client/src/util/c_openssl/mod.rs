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

//! TLS implementation based on [`Openssl`]
//!
//! [`Openssl`]: https://www.openssl.org/

#[macro_use]
mod foreign;
mod bio;
mod ffi;

pub(crate) mod error;
pub(crate) mod ssl;
pub(crate) mod stack;
pub(crate) mod x509;

pub mod adapter;
pub(crate) mod verify;

use core::ptr;
use std::sync::Once;

pub use adapter::{Cert, Certificate, TlsConfig, TlsConfigBuilder, TlsFileType, TlsVersion};
use error::ErrorStack;
use libc::c_int;
pub use verify::{PubKeyPins, PubKeyPinsBuilder};

pub(crate) use crate::util::c_openssl::ffi::callback::*;
use crate::util::c_openssl::ffi::OPENSSL_init_ssl;

/// Automatic loading of the libssl error strings. This option is a default
/// option.
pub(crate) const OPENSSL_INIT_LOAD_SSL_STRINGS: u64 = 0x00200000;

/// Checks null-pointer.
pub(crate) fn check_ptr<T>(ptr: *mut T) -> Result<*mut T, ErrorStack> {
    if ptr.is_null() {
        Err(ErrorStack::get())
    } else {
        Ok(ptr)
    }
}

/// Gets errors if the return value <= 0.
pub(crate) fn check_ret(r: c_int) -> Result<c_int, ErrorStack> {
    if r <= 0 {
        Err(ErrorStack::get())
    } else {
        Ok(r)
    }
}

/// Calls this function will explicitly initialise BOTH libcrypto and libssl.
pub(crate) fn ssl_init() {
    static SSL_INIT: Once = Once::new();
    let init_options = OPENSSL_INIT_LOAD_SSL_STRINGS;

    SSL_INIT.call_once(|| unsafe {
        OPENSSL_init_ssl(init_options, ptr::null_mut());
    })
}
