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

use core::fmt;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use std::io::{self, Read, Write};
use std::panic::resume_unwind;
use std::ptr;

use libc::c_int;

use super::{InternalError, Ssl, SslError, SslErrorCode, SslRef};
use crate::c_openssl::bio::{self, get_error, get_panic, get_stream_mut, get_stream_ref};
use crate::c_openssl::error::ErrorStack;
use crate::c_openssl::ffi::ssl::{SSL_connect, SSL_set_bio, SSL_shutdown};
use crate::c_openssl::foreign::Foreign;
use crate::util::base64::encode;
use crate::util::c_openssl::bio::BioMethod;
use crate::util::c_openssl::error::VerifyError;
use crate::util::c_openssl::error::VerifyKind::PubKeyPinning;
use crate::util::c_openssl::ffi::ssl::SSL;
use crate::util::c_openssl::ffi::x509::{i2d_X509_PUBKEY, X509_free, X509_get_X509_PUBKEY};
use crate::util::c_openssl::verify::sha256_digest;

/// A TLS session over a stream.
pub struct SslStream<S> {
    pub(crate) ssl: ManuallyDrop<Ssl>,
    method: ManuallyDrop<BioMethod>,
    pinned_pubkey: Option<String>,
    p: PhantomData<S>,
}

impl<S> SslStream<S> {
    pub(crate) fn get_error(&mut self, err: c_int) -> SslError {
        self.check_panic();
        let code = self.ssl.get_error(err);
        let internal = match code {
            SslErrorCode::SSL => {
                let e = ErrorStack::get();
                Some(InternalError::Ssl(e))
            }
            SslErrorCode::SYSCALL => {
                let error = ErrorStack::get();
                if error.errors().is_empty() {
                    self.get_bio_error().map(InternalError::Io)
                } else {
                    Some(InternalError::Ssl(error))
                }
            }
            SslErrorCode::WANT_WRITE | SslErrorCode::WANT_READ => {
                self.get_bio_error().map(InternalError::Io)
            }
            _ => None,
        };
        SslError { code, internal }
    }

    fn check_panic(&mut self) {
        if let Some(err) = unsafe { get_panic::<S>(self.ssl.get_raw_bio()) } {
            resume_unwind(err)
        }
    }

    fn get_bio_error(&mut self) -> Option<io::Error> {
        unsafe { get_error::<S>(self.ssl.get_raw_bio()) }
    }

    pub(crate) fn get_ref(&self) -> &S {
        unsafe {
            let bio = self.ssl.get_raw_bio();
            get_stream_ref(bio)
        }
    }

    pub(crate) fn get_mut(&mut self) -> &mut S {
        unsafe {
            let bio = self.ssl.get_raw_bio();
            get_stream_mut(bio)
        }
    }

    pub(crate) fn ssl(&self) -> &SslRef {
        &self.ssl
    }
}

impl<S> fmt::Debug for SslStream<S>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stream[{:?}], {:?}", &self.get_ref(), &self.ssl())
    }
}

impl<S> Drop for SslStream<S> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.ssl);
            ManuallyDrop::drop(&mut self.method);
        }
    }
}

impl<S: Read + Write> SslStream<S> {
    pub(crate) fn ssl_read(&mut self, buf: &[u8]) -> Result<usize, SslError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let ret = self.ssl.read(buf);
        if ret > 0 {
            Ok(ret as usize)
        } else {
            Err(self.get_error(ret))
        }
    }

    pub(crate) fn ssl_write(&mut self, buf: &[u8]) -> Result<usize, SslError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let ret = self.ssl.write(buf);
        if ret > 0 {
            Ok(ret as usize)
        } else {
            Err(self.get_error(ret))
        }
    }

    pub(crate) fn new_base(
        ssl: Ssl,
        stream: S,
        pinned_pubkey: Option<String>,
    ) -> Result<Self, ErrorStack> {
        unsafe {
            let (bio, method) = bio::new(stream)?;
            SSL_set_bio(ssl.as_ptr(), bio, bio);

            Ok(SslStream {
                ssl: ManuallyDrop::new(ssl),
                method: ManuallyDrop::new(method),
                pinned_pubkey,
                p: PhantomData,
            })
        }
    }

    pub(crate) fn connect(&mut self) -> Result<(), SslError> {
        let ret = unsafe { SSL_connect(self.ssl.as_ptr()) };
        if ret > 0 {
            match &self.pinned_pubkey {
                None => {}
                Some(key) => {
                    verify_server_cert(self.ssl.as_ptr(), key.as_str())?;
                }
            }
            Ok(())
        } else {
            Err(self.get_error(ret))
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<ShutdownResult, SslError> {
        unsafe {
            match SSL_shutdown(self.ssl.as_ptr()) {
                0 => Ok(ShutdownResult::Sent),
                1 => Ok(ShutdownResult::Received),
                n => Err(self.get_error(n)),
            }
        }
    }
}

impl<S: Read + Write> Read for SslStream<S> {
    // ssl_read
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.ssl_read(buf) {
                Ok(n) => return Ok(n),
                // The TLS/SSL peer has closed the connection for writing by sending
                // the close_notify alert. No more data can be read.
                // Does not necessarily indicate that the underlying transport has been closed.
                Err(ref e) if e.code == SslErrorCode::ZERO_RETURN => return Ok(0),
                // A non-recoverable, fatal error in the SSL library occurred, usually a protocol
                // error.
                Err(ref e) if e.code == SslErrorCode::SYSCALL && e.get_io_error().is_none() => {
                    return Ok(0)
                }
                // When the last operation was a read operation from a nonblocking BIO.
                Err(ref e) if e.code == SslErrorCode::WANT_READ && e.get_io_error().is_none() => {}
                // Other error.
                Err(err) => {
                    return Err(err
                        .into_io_error()
                        .unwrap_or_else(|err| io::Error::new(io::ErrorKind::Other, err)))
                }
            };
        }
    }
}

impl<S: Read + Write> Write for SslStream<S> {
    // ssl_write
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.ssl_write(buf) {
                Ok(n) => return Ok(n),
                // When the last operation was a read operation from a nonblocking BIO.
                Err(ref e) if e.code == SslErrorCode::WANT_READ && e.get_io_error().is_none() => {}
                Err(err) => {
                    return Err(err
                        .into_io_error()
                        .unwrap_or_else(|err| io::Error::new(io::ErrorKind::Other, err)));
                }
            }
        }
    }

    // S.flush()
    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// An SSL stream midway through the handshake process.
#[derive(Debug)]
pub(crate) struct MidHandshakeSslStream<S> {
    pub(crate) _stream: SslStream<S>,
    pub(crate) error: SslError,
}

impl<S> MidHandshakeSslStream<S> {
    pub(crate) fn error(&self) -> &SslError {
        &self.error
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShutdownResult {
    Sent,
    Received,
}

// TODO The SSLError thrown here is meaningless and has no information.
fn verify_server_cert(ssl: *const SSL, pinned_key: &str) -> Result<(), SslError> {
    #[cfg(feature = "c_openssl_3_0")]
    use crate::util::c_openssl::ffi::ssl::SSL_get1_peer_certificate;
    #[cfg(feature = "c_openssl_1_1")]
    use crate::util::c_openssl::ffi::ssl::SSL_get_peer_certificate;

    let certificate = unsafe {
        #[cfg(feature = "c_openssl_3_0")]
        {
            SSL_get1_peer_certificate(ssl)
        }
        #[cfg(feature = "c_openssl_1_1")]
        {
            SSL_get_peer_certificate(ssl)
        }
    };
    if certificate.is_null() {
        return Err(SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        });
    }

    let size_1 = unsafe { i2d_X509_PUBKEY(X509_get_X509_PUBKEY(certificate), ptr::null_mut()) };
    if size_1 < 1 {
        unsafe { X509_free(certificate) };
        return Err(SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        });
    }
    let key = vec![0u8; size_1 as usize];
    let size_2 = unsafe { i2d_X509_PUBKEY(X509_get_X509_PUBKEY(certificate), &mut key.as_ptr()) };

    if size_1 != size_2 || size_2 <= 0 {
        unsafe { X509_free(certificate) };
        return Err(SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        });
    }

    // sha256 length.
    let mut digest = [0u8; 32];
    unsafe { sha256_digest(key.as_slice(), size_2, &mut digest)? }
    let base64_digest = encode(&digest);

    let mut user_bytes = pinned_key.as_bytes();

    let mut begin;
    let mut end;
    let prefix = b"sha256//";
    let suffix = b";sha256//";
    while !user_bytes.is_empty() {
        begin = match user_bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        {
            None => {
                break;
            }
            Some(index) => index + 8,
        };
        end = match user_bytes
            .windows(suffix.len())
            .position(|window| window == suffix)
        {
            None => user_bytes.len(),
            Some(index) => index,
        };

        let bytes = &user_bytes[begin..end];
        if bytes.eq(base64_digest.as_slice()) {
            unsafe { X509_free(certificate) };
            return Ok(());
        }

        if end != user_bytes.len() {
            user_bytes = &user_bytes[end + 1..];
        } else {
            user_bytes = &user_bytes[end..];
        }
    }

    unsafe { X509_free(certificate) };
    Err(SslError {
        code: SslErrorCode::SSL,
        internal: Some(InternalError::User(VerifyError::from_msg(
            PubKeyPinning,
            "Pinned public key verification failed.",
        ))),
    })
}
