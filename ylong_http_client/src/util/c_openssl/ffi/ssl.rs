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
    bio::BIO,
    x509::{C_X509, X509_STORE},
};
use libc::{c_char, c_int, c_long, c_uchar, c_uint, c_void};

/// This is the global context structure which is created by a server or client
/// once per program life-time and which holds mainly default values for the
/// `SSL` structures which are later created for the connections.
pub(crate) enum SSL_CTX {}

// for `SSL_CTX`
extern "C" {
    /// Creates a new `SSL_CTX` object, which holds various configuration and
    /// data relevant to SSL/TLS or DTLS session establishment. \
    /// Returns `Null` if failed.
    pub(crate) fn SSL_CTX_new(method: *const SSL_METHOD) -> *mut SSL_CTX;

    /// Frees an allocated `SSL_CTX` object
    pub(crate) fn SSL_CTX_free(ctx: *mut SSL_CTX);

    /// Increments the reference count for an existing `SSL_CTX` structure.
    /// Returns 1 for success and 0 for failure
    pub(crate) fn SSL_CTX_up_ref(x: *mut SSL_CTX) -> c_int;

    /// Internal handling functions for SSL_CTX objects.
    pub(crate) fn SSL_CTX_ctrl(
        ctx: *mut SSL_CTX,
        cmd: c_int,
        larg: c_long,
        parg: *mut c_void,
    ) -> c_long;

    /// Set default locations for trusted CA certificates.
    pub(crate) fn SSL_CTX_load_verify_locations(
        ctx: *mut SSL_CTX,
        CAfile: *const c_char,
        CApath: *const c_char,
    ) -> c_int;

    /// Sets the list of available ciphers (TLSv1.2 and below) for ctx using the
    /// control string str.\
    /// This function does not impact TLSv1.3 ciphersuites.
    pub(crate) fn SSL_CTX_set_cipher_list(ssl: *mut SSL_CTX, s: *const c_char) -> c_int;

    /// Uses to configure the available TLSv1.3 ciphersuites for ctx.
    pub(crate) fn SSL_CTX_set_ciphersuites(ctx: *mut SSL_CTX, str: *const c_char) -> c_int;

    /// Loads the first certificate stored in file into ctx.
    /// The formatting type of the certificate must be specified from the known
    /// types SSL_FILETYPE_PEM, SSL_FILETYPE_ASN1.
    pub(crate) fn SSL_CTX_use_certificate_file(
        ctx: *mut SSL_CTX,
        cert_file: *const c_char,
        file_type: c_int,
    ) -> c_int;

    /// Loads a certificate chain from file into ctx. The certificates must be in
    /// PEM format and must be sorted starting with the subject's certificate (actual
    /// client or server certificate), followed by intermediate CA certificates
    /// if applicable, and ending at the highest level (root) CA.
    pub(crate) fn SSL_CTX_use_certificate_chain_file(
        ctx: *mut SSL_CTX,
        cert_chain_file: *const c_char,
    ) -> c_int;

    /// Loads the certificate `cert` into ctx.\
    /// The rest of the certificates needed to form the complete certificate chain
    /// can be specified using the `SSL_CTX_add_extra_chain_cert` function
    pub(crate) fn SSL_CTX_use_certificate(ctx: *mut SSL_CTX, cert: *mut C_X509) -> c_int;

    /// Client sets the list of protocols available to be negotiated.
    pub(crate) fn SSL_CTX_set_alpn_protos(
        s: *mut SSL_CTX,
        data: *const c_uchar,
        len: c_uint,
    ) -> c_int;

    /// Sets/replaces the certificate verification storage of ctx to/with store.
    /// If another X509_STORE object is currently set in ctx, it will be X509_STORE_free()ed.
    pub(crate) fn SSL_CTX_set_cert_store(ctx: *mut SSL_CTX, store: *mut X509_STORE);
}

/// This is the main SSL/TLS structure which is created by a server or client per
/// established connection. This actually is the core structure in the SSL API.
/// At run-time the application usually deals with this structure which has links
/// to mostly all other structures.
pub(crate) enum SSL {}

// for `SSL`
extern "C" {
    /// Creates a new `SSL` structure which is needed to hold the data for a
    /// TLS/SSL connection. \
    /// Returns `Null` if failed.
    pub(crate) fn SSL_new(ctx: *mut SSL_CTX) -> *mut SSL;

    pub(crate) fn SSL_free(ssl: *mut SSL);

    /// Obtains result code for TLS/SSL I/O operation.\
    /// SSL_get_error() must be used in the same thread that performed the TLS/SSL
    /// I/O operation, and no other OpenSSL function calls should appear in between.
    pub(crate) fn SSL_get_error(ssl: *const SSL, ret: c_int) -> c_int;

    /// Returns an abbreviated string indicating the current state of the SSL object ssl.
    pub(crate) fn SSL_state_string_long(ssl: *const SSL) -> *const c_char;

    /// Returns the result of the verification of the X509 certificate presented by the peer, if any.
    pub(crate) fn SSL_get_verify_result(ssl: *const SSL) -> c_long;

    pub(crate) fn SSL_set_bio(ssl: *mut SSL, rbio: *mut BIO, wbio: *mut BIO);

    pub(crate) fn SSL_get_rbio(ssl: *const SSL) -> *mut BIO;

    pub(crate) fn SSL_read(ssl: *mut SSL, buf: *mut c_void, num: c_int) -> c_int;

    pub(crate) fn SSL_write(ssl: *mut SSL, buf: *const c_void, num: c_int) -> c_int;

    pub(crate) fn SSL_connect(ssl: *mut SSL) -> c_int;

    pub(crate) fn SSL_shutdown(ssl: *mut SSL) -> c_int;
}

/// This is a dispatch structure describing the internal ssl library methods/functions
/// which implement the various protocol versions (SSLv3 TLSv1, ...).
/// It's needed to create an `SSL_CTX`.
pub(crate) enum SSL_METHOD {}

// for `SSL_METHOD`
extern "C" {
    /// Is the general-purpose version-flexible SSL/TLS methods. The actual protocol
    /// version used will be negotiated to the highest version mutually supported
    /// by the client and the server.
    pub(crate) fn TLS_client_method() -> *const SSL_METHOD;
}
