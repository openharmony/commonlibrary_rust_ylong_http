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

use crate::c_openssl::error::ErrorStack;
use core::fmt;
use libc::c_int;
use std::{error::Error, io};

use super::MidHandshakeSslStream;

#[derive(Debug)]
pub(crate) struct SslError {
    pub(crate) code: SslErrorCode,
    pub(crate) internal: Option<InternalError>,
}

#[derive(Debug)]
pub(crate) enum InternalError {
    Io(io::Error),
    Ssl(ErrorStack),
}

impl SslError {
    pub(crate) fn code(&self) -> SslErrorCode {
        self.code
    }

    pub(crate) fn into_io_error(self) -> Result<io::Error, SslError> {
        match self.internal {
            Some(InternalError::Io(e)) => Ok(e),
            _ => Err(self),
        }
    }

    pub(crate) fn get_io_error(&self) -> Option<&io::Error> {
        match self.internal {
            Some(InternalError::Io(ref e)) => Some(e),
            _ => None,
        }
    }
}

impl Error for SslError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.internal {
            Some(InternalError::Io(ref e)) => Some(e),
            Some(InternalError::Ssl(ref e)) => Some(e),
            None => None,
        }
    }
}

impl fmt::Display for SslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            SslErrorCode::ZERO_RETURN => write!(f, "SSL session has been closed"),
            SslErrorCode::SYSCALL => {
                if let Some(InternalError::Io(e)) = &self.internal {
                    write!(f, "SslCode[{}], IO Error: {}", self.code, e)
                } else {
                    write!(f, "SslCode[{}], Unexpected EOF", self.code)
                }
            }
            SslErrorCode::SSL => {
                if let Some(InternalError::Ssl(e)) = &self.internal {
                    write!(f, "ErrorStack: {e}")
                } else {
                    write!(f, "SslCode: [{}]", self.code)
                }
            }
            SslErrorCode::WANT_READ => {
                if let Some(InternalError::Io(e)) = &self.internal {
                    write!(f, "SslCode[{}], IO Error: {}", self.code, e)
                } else {
                    write!(
                        f,
                        "SslCode[{}], Read operation should be retried",
                        self.code
                    )
                }
            }
            SslErrorCode::WANT_WRITE => {
                if let Some(InternalError::Io(e)) = &self.internal {
                    write!(f, "SslCode[{}], IO Error: {}", self.code, e)
                } else {
                    write!(
                        f,
                        "SslCode[{}], Write operation should be retried",
                        self.code
                    )
                }
            }
            _ => {
                write!(f, "Unknown SslCode[{}]", self.code)
            }
        }
    }
}

const SSL_ERROR_SSL: c_int = 1;
const SSL_ERROR_SYSCALL: c_int = 5;
const SSL_ERROR_WANT_READ: c_int = 2;
const SSL_ERROR_WANT_WRITE: c_int = 3;
const SSL_ERROR_ZERO_RETURN: c_int = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct SslErrorCode(c_int);

impl SslErrorCode {
    pub(crate) const ZERO_RETURN: SslErrorCode = SslErrorCode(SSL_ERROR_ZERO_RETURN);
    pub(crate) const WANT_READ: SslErrorCode = SslErrorCode(SSL_ERROR_WANT_READ);
    pub(crate) const WANT_WRITE: SslErrorCode = SslErrorCode(SSL_ERROR_WANT_WRITE);
    pub(crate) const SYSCALL: SslErrorCode = SslErrorCode(SSL_ERROR_SYSCALL);
    pub(crate) const SSL: SslErrorCode = SslErrorCode(SSL_ERROR_SSL);

    pub(crate) fn from_int(err: c_int) -> SslErrorCode {
        SslErrorCode(err)
    }
}

impl fmt::Display for SslErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub(crate) enum HandshakeError<S> {
    SetupFailure(ErrorStack),
    Failure(MidHandshakeSslStream<S>),
    WouldBlock(MidHandshakeSslStream<S>),
}

impl<S> From<ErrorStack> for HandshakeError<S> {
    fn from(e: ErrorStack) -> HandshakeError<S> {
        HandshakeError::SetupFailure(e)
    }
}

impl<S: fmt::Debug> Error for HandshakeError<S> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            HandshakeError::SetupFailure(ref e) => Some(e),
            HandshakeError::Failure(ref s) | HandshakeError::WouldBlock(ref s) => Some(s.error()),
        }
    }
}

impl<S: fmt::Debug> fmt::Display for HandshakeError<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            HandshakeError::SetupFailure(ref e) => write!(f, "Stream setup failed: {e}")?,
            HandshakeError::Failure(ref s) => {
                write!(f, "Handshake failed: {}", s.error())?;
            }
            HandshakeError::WouldBlock(ref s) => {
                write!(f, "Handshake was interrupted: {}", s.error())?;
            }
        }
        Ok(())
    }
}
