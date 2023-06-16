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

use libc::{c_char, c_long, c_uchar};

pub(crate) enum C_X509 {}

// for `C_X509`
extern "C" {
    pub(crate) fn X509_free(a: *mut C_X509);

    /// Returns a human readable error string for verification error n.
    pub(crate) fn X509_verify_cert_error_string(n: c_long) -> *const c_char;

    /// Attempts to decode len bytes at *ppin.\
    /// If successful a pointer to the TYPE structure is returned and *ppin is
    /// incremented to the byte following the parsed data.
    pub(crate) fn d2i_X509(
        a: *mut *mut C_X509,
        pp: *mut *const c_uchar,
        length: c_long,
    ) -> *mut C_X509;
}

pub(crate) enum X509_STORE {}

// for `X509_STORE`
extern "C" {
    /// Returns a new `X509_STORE`.
    pub(crate) fn X509_STORE_new() -> *mut X509_STORE;

    /// Frees up a single `X509_STORE` object.
    pub(crate) fn X509_STORE_free(store: *mut X509_STORE);
}
