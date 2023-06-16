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

use super::{InternalError, Ssl, SslError, SslErrorCode, SslRef};
use crate::{
    c_openssl::{
        bio::{self, get_error, get_panic, get_stream_mut, get_stream_ref},
        error::ErrorStack,
        ffi::ssl::{SSL_connect, SSL_set_bio, SSL_shutdown},
        foreign::Foreign,
    },
    util::c_openssl::bio::BioMethod,
};
use core::{fmt, marker::PhantomData, mem::ManuallyDrop};
use libc::c_int;
use std::{
    io::{self, Read, Write},
    panic::resume_unwind,
};

/// A TLS session over a stream.
pub struct SslStream<S> {
    pub(crate) ssl: ManuallyDrop<Ssl>,
    method: ManuallyDrop<BioMethod>,
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
    pub(crate) fn ssl_read(&mut self, buf: &mut [u8]) -> Result<usize, SslError> {
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

    pub(crate) fn new_base(ssl: Ssl, stream: S) -> Result<Self, ErrorStack> {
        unsafe {
            let (bio, method) = bio::new(stream)?;
            SSL_set_bio(ssl.as_ptr(), bio, bio);

            Ok(SslStream {
                ssl: ManuallyDrop::new(ssl),
                method: ManuallyDrop::new(method),
                p: PhantomData,
            })
        }
    }

    pub(crate) fn connect(&mut self) -> Result<(), SslError> {
        let ret = unsafe { SSL_connect(self.ssl.as_ptr()) };
        if ret > 0 {
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
                // A non-recoverable, fatal error in the SSL library occurred, usually a protocol error.
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
