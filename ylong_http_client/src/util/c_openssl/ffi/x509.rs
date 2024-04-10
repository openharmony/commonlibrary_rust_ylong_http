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

use libc::{c_char, c_int, c_long, c_uchar, c_uint, size_t};

pub(crate) enum EVP_PKEY {}

extern "C" {
    pub(crate) fn EVP_PKEY_free(ctx: *mut EVP_PKEY);
}

pub(crate) enum EVP_MD_CTX {}

pub(crate) enum EVP_MD {}

extern "C" {
    pub(crate) fn EVP_MD_CTX_new() -> *mut EVP_MD_CTX;

    pub(crate) fn EVP_sha256() -> *mut EVP_MD;

    pub(crate) fn EVP_DigestInit(ctx: *mut EVP_MD_CTX, md: *mut EVP_MD) -> c_int;

    pub(crate) fn EVP_MD_CTX_free(ctx: *mut EVP_MD_CTX);

    pub(crate) fn EVP_DigestUpdate(ctx: *mut EVP_MD_CTX, buf: *const c_uchar, cnt: c_int) -> c_int;

    pub(crate) fn EVP_DigestFinal_ex(
        ctx: *mut EVP_MD_CTX,
        buf: *const c_uchar,
        start: *const c_uint,
    );
}

pub(crate) enum C_X509 {}

// for `C_X509`
extern "C" {
    pub(crate) fn X509_free(a: *mut C_X509);

    /// Returns a human readable error string for verification error n.
    pub(crate) fn X509_verify_cert_error_string(n: c_long) -> *const c_char;

    /// Attempts to decode len bytes at *ppin.
    /// If successful a pointer to the TYPE structure is returned and *ppin is
    /// incremented to the byte following the parsed data.
    pub(crate) fn d2i_X509(
        a: *mut *mut C_X509,
        pp: *mut *const c_uchar,
        length: c_long,
    ) -> *mut C_X509;

    pub(crate) fn X509_get_version(a: *const C_X509) -> c_long;

    pub(crate) fn X509_get_subject_name(a: *const C_X509) -> *mut X509_NAME;

    pub(crate) fn X509_get_issuer_name(a: *const C_X509) -> *mut X509_NAME;

    pub(crate) fn X509_NAME_oneline(
        a: *mut X509_NAME,
        buf: *mut c_char,
        size: c_int,
    ) -> *mut c_char;

    pub(crate) fn X509_get_pubkey(a: *mut C_X509) -> *mut EVP_PKEY;

    pub(crate) fn X509_verify(a: *mut C_X509, pkey: *mut EVP_PKEY) -> c_int;

    pub(crate) fn X509_up_ref(x: *mut C_X509) -> c_int;
}
pub(crate) enum X509_NAME {}

extern "C" {
    pub(crate) fn X509_NAME_free(name: *mut X509_NAME);
}
pub(crate) enum X509_STORE {}

// for `X509_STORE`
extern "C" {
    /// Returns a new `X509_STORE`.
    pub(crate) fn X509_STORE_new() -> *mut X509_STORE;

    /// Frees up a single `X509_STORE` object.
    pub(crate) fn X509_STORE_free(store: *mut X509_STORE);

    /// Adds the respective object to the X509_STORE's local storage.
    pub(crate) fn X509_STORE_add_cert(store: *mut X509_STORE, x: *mut C_X509) -> c_int;

    /// Adds the respective object of file paths to the X509_STORE's local
    /// storage.
    #[cfg(feature = "c_openssl_3_0")]
    pub(crate) fn X509_STORE_load_path(store: *mut X509_STORE, x: *const c_char) -> c_int;
}

pub(crate) enum X509_STORE_CTX {}

extern "C" {
    pub(crate) fn X509_STORE_CTX_free(ctx: *mut X509_STORE_CTX);

    pub(crate) fn X509_STORE_CTX_get0_cert(ctx: *const X509_STORE_CTX) -> *mut C_X509;
}

pub(crate) enum X509_VERIFY_PARAM {}

// for `X509_VERIFY_PARAM`
extern "C" {
    pub(crate) fn X509_VERIFY_PARAM_free(param: *mut X509_VERIFY_PARAM);

    pub(crate) fn X509_VERIFY_PARAM_set_hostflags(param: *mut X509_VERIFY_PARAM, flags: c_uint);

    /// If name is NUL-terminated, namelen may be zero, otherwise namelen must
    /// be set to the length of name.
    pub(crate) fn X509_VERIFY_PARAM_set1_host(
        param: *mut X509_VERIFY_PARAM,
        name: *const c_char,
        namelen: size_t,
    ) -> c_int;

    /// The ip argument is in binary format, in network byte-order and iplen
    /// must be set to 4 for IPv4 and 16 for IPv6.
    pub(crate) fn X509_VERIFY_PARAM_set1_ip(
        param: *mut X509_VERIFY_PARAM,
        ip: *const c_uchar,
        iplen: size_t,
    ) -> c_int;
}

pub(crate) enum STACK_X509 {}
pub(crate) enum X509_PUBKEY {}

extern "C" {
    pub(crate) fn X509_get_X509_PUBKEY(x509: *mut C_X509) -> *mut X509_PUBKEY;

    pub(crate) fn X509_PUBKEY_free(x509: *mut X509_PUBKEY);

    pub(crate) fn i2d_X509_PUBKEY(pubkey: *const X509_PUBKEY, buf: *mut *const c_uchar) -> c_int;
}
