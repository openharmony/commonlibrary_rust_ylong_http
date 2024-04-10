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
use std::error::Error;
use std::io;

use libc::c_int;

use super::MidHandshakeSslStream;
use crate::c_openssl::error::ErrorStack;
use crate::util::c_openssl::error::VerifyError;

#[derive(Debug)]
pub(crate) struct SslError {
    pub(crate) code: SslErrorCode,
    pub(crate) internal: Option<InternalError>,
}

#[derive(Debug)]
pub(crate) enum InternalError {
    Io(io::Error),
    Ssl(ErrorStack),
    User(VerifyError),
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
            Some(InternalError::User(ref e)) => Some(e),
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
    #[allow(dead_code)]
    Failure(MidHandshakeSslStream<S>),
    #[allow(dead_code)]
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

#[cfg(test)]
mod ut_ssl_error {
    use std::error::Error;
    use std::io;

    use crate::util::c_openssl::error::VerifyKind::PubKeyPinning;
    use crate::util::c_openssl::error::{ErrorStack, VerifyError};
    use crate::util::c_openssl::ssl::{InternalError, SslError, SslErrorCode};

    /// UT test cases for `SslErrorCode::from_int`.
    ///
    /// # Brief
    /// 1. Creates a `SslErrorCode` by calling `Redirect::from_int`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_ssl_error_code_from_int() {
        let v_6 = SslErrorCode::from_int(6);
        assert_eq!(v_6, SslErrorCode::ZERO_RETURN);
        assert!(v_6 > SslErrorCode::WANT_READ);
        assert_eq!(v_6.clone(), v_6);
        assert_eq!(format!("{v_6}"), "6");
    }

    /// UT test cases for `SslError::into_io_error`.
    ///
    /// # Brief
    /// 1. Creates a `SslErrorCode`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_ssl_error_into_io_error() {
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert!(ssl_error.get_io_error().is_some());
        assert!(ssl_error.into_io_error().is_ok());
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: None,
        };
        assert!(ssl_error.get_io_error().is_none());
        assert!(ssl_error.into_io_error().is_err());
    }

    /// UT test cases for `SslError::source`.
    ///
    /// # Brief
    /// 1. Creates a `SslErrorCode`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_ssl_error_source() {
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert!(ssl_error.source().is_some());
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        };
        assert!(ssl_error.source().is_some());
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: None,
        };
        assert!(ssl_error.source().is_none());
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: Some(InternalError::User(VerifyError::from_msg(
                PubKeyPinning,
                "error",
            ))),
        };
        assert!(ssl_error.source().is_some());
    }

    /// UT test cases for `SslError::fmt`.
    ///
    /// # Brief
    /// 1. Creates a `SslErrorCode`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_ssl_error_fmt() {
        let ssl_error = SslError {
            code: SslErrorCode::ZERO_RETURN,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert_eq!(format!("{}", ssl_error), "SSL session has been closed");

        let ssl_error = SslError {
            code: SslErrorCode::SYSCALL,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert_eq!(
            format!("{}", ssl_error),
            format!(
                "SslCode[5], IO Error: {}",
                io::Error::from(io::ErrorKind::BrokenPipe)
            )
        );

        let ssl_error = SslError {
            code: SslErrorCode::SYSCALL,
            internal: None,
        };
        assert_eq!(
            format!("{}", ssl_error),
            "SslCode[5], Unexpected EOF".to_string()
        );

        let ssl_error = SslError {
            code: SslErrorCode::SSL,
            internal: None,
        };
        assert_eq!(format!("{}", ssl_error), "SslCode: [1]".to_string());

        let error_stack = ErrorStack::get();
        let ssl_error = SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(error_stack.clone())),
        };
        let error_stack = format!("{error_stack}");
        assert_eq!(
            format!("{}", ssl_error),
            format!("ErrorStack: {}", error_stack)
        );

        let ssl_error = SslError {
            code: SslErrorCode::WANT_READ,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert_eq!(
            format!("{}", ssl_error),
            format!(
                "SslCode[2], IO Error: {}",
                io::Error::from(io::ErrorKind::BrokenPipe)
            )
        );

        let ssl_error = SslError {
            code: SslErrorCode::WANT_READ,
            internal: None,
        };
        assert_eq!(
            format!("{}", ssl_error),
            "SslCode[2], Read operation should be retried".to_string()
        );

        let ssl_error = SslError {
            code: SslErrorCode::WANT_WRITE,
            internal: Some(InternalError::Io(io::Error::from(
                io::ErrorKind::BrokenPipe,
            ))),
        };
        assert_eq!(
            format!("{}", ssl_error),
            format!(
                "SslCode[3], IO Error: {}",
                io::Error::from(io::ErrorKind::BrokenPipe)
            )
        );

        let ssl_error = SslError {
            code: SslErrorCode::WANT_WRITE,
            internal: None,
        };
        assert_eq!(
            format!("{}", ssl_error),
            "SslCode[3], Write operation should be retried".to_string()
        );

        let ssl_error = SslError {
            code: SslErrorCode::from_int(15),
            internal: None,
        };
        assert_eq!(format!("{}", ssl_error), "Unknown SslCode[15]".to_string());
    }

    /// UT test cases for `Debug` of `SslError`.
    ///
    /// # Brief
    /// 1. Creates a `SslErrorCode`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_ssl_error_debug() {
        let ssl_error = SslError {
            code: SslErrorCode::from_int(15),
            internal: None,
        };
        assert_eq!(
            format!("{:?}", ssl_error),
            "SslError { code: SslErrorCode(15), internal: None }".to_string()
        );
    }

    /// UT test cases for `Debug` of `InternalError`.
    ///
    /// # Brief
    /// 1. Creates a `InternalError`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_internal_error_debug() {
        let internal_error = InternalError::Ssl(ErrorStack::get());
        assert_eq!(
            format!("{:?}", internal_error),
            "Ssl(ErrorStack([]))".to_string()
        );
    }
}
