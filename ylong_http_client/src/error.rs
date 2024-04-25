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

//! Definition of `HttpClientErrors` which includes errors that may occur in
//! this crate.

use core::fmt::{Debug, Display, Formatter};
use std::sync::Once;
use std::{error, io};

/// The structure encapsulates errors that can be encountered when working with
/// the HTTP client.
///
/// # Examples
///
/// ```
/// use ylong_http_client::HttpClientError;
///
/// let error = HttpClientError::user_aborted();
/// ```
pub struct HttpClientError {
    kind: ErrorKind,
    cause: Cause,
}

impl HttpClientError {
    /// Creates a `UserAborted` error.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::HttpClientError;
    ///
    /// let user_aborted = HttpClientError::user_aborted();
    /// ```
    pub fn user_aborted() -> Self {
        Self {
            kind: ErrorKind::UserAborted,
            cause: Cause::NoReason,
        }
    }

    /// Creates an `Other` error.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::HttpClientError;
    ///
    /// fn error(error: std::io::Error) {
    ///     let other = HttpClientError::other(error);
    /// }
    /// ```
    pub fn other<T>(cause: T) -> Self
    where
        T: Into<Box<dyn error::Error + Send + Sync>>,
    {
        Self {
            kind: ErrorKind::Other,
            cause: Cause::Other(cause.into()),
        }
    }

    /// Gets the `ErrorKind` of this `HttpClientError`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::{ErrorKind, HttpClientError};
    ///
    /// let user_aborted = HttpClientError::user_aborted();
    /// assert_eq!(user_aborted.error_kind(), ErrorKind::UserAborted);
    /// ```
    pub fn error_kind(&self) -> ErrorKind {
        self.kind
    }

    /// Gets the `io::Error` if this `HttpClientError` comes from an
    /// `io::Error`.
    ///
    /// Returns `None` if the `HttpClientError` doesn't come from an
    /// `io::Error`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::HttpClientError;
    ///
    /// let error = HttpClientError::user_aborted().io_error();
    /// ```
    pub fn io_error(&self) -> Option<&io::Error> {
        match self.cause {
            Cause::Io(ref io) => Some(io),
            _ => None,
        }
    }
}

impl HttpClientError {
    pub(crate) fn from_error<T>(kind: ErrorKind, err: T) -> Self
    where
        T: Into<Box<dyn error::Error + Send + Sync>>,
    {
        Self {
            kind,
            cause: Cause::Other(err.into()),
        }
    }

    pub(crate) fn from_str(kind: ErrorKind, msg: &'static str) -> Self {
        Self {
            kind,
            cause: Cause::Msg(msg),
        }
    }

    pub(crate) fn from_io_error(kind: ErrorKind, err: io::Error) -> Self {
        Self {
            kind,
            cause: Cause::Io(err),
        }
    }
}

impl Debug for HttpClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let mut builder = f.debug_struct("HttpClientError");
        builder.field("ErrorKind", &self.kind);
        builder.field("Cause", &self.cause);
        builder.finish()
    }
}

impl Display for HttpClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.kind.as_str())?;
        write!(f, ": {}", self.cause)?;
        Ok(())
    }
}

impl error::Error for HttpClientError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        static mut USER_ABORTED: Option<Box<dyn error::Error>> = None;
        static ONCE: Once = Once::new();

        ONCE.call_once(|| {
            unsafe { USER_ABORTED = Some(Box::new(HttpClientError::user_aborted())) };
        });

        if self.kind == ErrorKind::UserAborted {
            return unsafe { USER_ABORTED.as_ref().map(|e| e.as_ref()) };
        }
        None
    }
}

/// Error kinds which can indicate the type of a `HttpClientError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Errors for decoding response body.
    BodyDecode,

    /// Errors for transferring request body or response body.
    BodyTransfer,

    /// Errors for using various builder.
    Build,

    /// Errors for connecting to a server.
    Connect,

    /// Errors for upgrading a connection.
    ConnectionUpgrade,

    /// Other error kinds.
    Other,

    /// Errors for following redirect.
    Redirect,

    /// Errors for sending a request.
    Request,

    /// Errors for reaching a timeout.
    Timeout,

    /// User raised errors.
    UserAborted,
}

impl ErrorKind {
    /// Gets the string info of this `ErrorKind`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::ErrorKind;
    ///
    /// assert_eq!(ErrorKind::UserAborted.as_str(), "User Aborted Error");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BodyDecode => "Body Decode Error",
            Self::BodyTransfer => "Body Transfer Error",
            Self::Build => "Build Error",
            Self::Connect => "Connect Error",
            Self::ConnectionUpgrade => "Connection Upgrade Error",
            Self::Other => "Other Error",
            Self::Redirect => "Redirect Error",
            Self::Request => "Request Error",
            Self::Timeout => "Timeout Error",
            Self::UserAborted => "User Aborted Error",
        }
    }
}

pub(crate) enum Cause {
    NoReason,
    Io(io::Error),
    Msg(&'static str),
    Other(Box<dyn error::Error + Send + Sync>),
}

impl Debug for Cause {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoReason => write!(f, "No reason"),
            Self::Io(err) => Debug::fmt(err, f),
            Self::Msg(msg) => write!(f, "{}", msg),
            Self::Other(err) => Debug::fmt(err, f),
        }
    }
}

impl Display for Cause {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoReason => write!(f, "No reason"),
            Self::Io(err) => Display::fmt(err, f),
            Self::Msg(msg) => write!(f, "{}", msg),
            Self::Other(err) => Display::fmt(err, f),
        }
    }
}

macro_rules! err_from_other {
    ($kind: ident, $err: expr) => {{
        use crate::error::{ErrorKind, HttpClientError};

        Err(HttpClientError::from_error(ErrorKind::$kind, $err))
    }};
}

macro_rules! err_from_io {
    ($kind: ident, $err: expr) => {{
        use crate::error::{ErrorKind, HttpClientError};

        Err(HttpClientError::from_io_error(ErrorKind::$kind, $err))
    }};
}

macro_rules! err_from_msg {
    ($kind: ident, $msg: literal) => {{
        use crate::error::{ErrorKind, HttpClientError};

        Err(HttpClientError::from_str(ErrorKind::$kind, $msg))
    }};
}

#[cfg(test)]
mod ut_util_error {
    use std::io;

    use crate::{ErrorKind, HttpClientError};

    /// UT test cases for `ErrorKind::as_str`.
    ///
    /// # Brief
    /// 1. Transfer ErrorKind to str a by calling `ErrorKind::as_str`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_as_str() {
        assert_eq!(ErrorKind::BodyDecode.as_str(), "Body Decode Error");
        assert_eq!(ErrorKind::BodyTransfer.as_str(), "Body Transfer Error");
        assert_eq!(ErrorKind::Build.as_str(), "Build Error");
        assert_eq!(ErrorKind::Connect.as_str(), "Connect Error");
        assert_eq!(
            ErrorKind::ConnectionUpgrade.as_str(),
            "Connection Upgrade Error"
        );
        assert_eq!(ErrorKind::Other.as_str(), "Other Error");
        assert_eq!(ErrorKind::Redirect.as_str(), "Redirect Error");
        assert_eq!(ErrorKind::Request.as_str(), "Request Error");
        assert_eq!(ErrorKind::Timeout.as_str(), "Timeout Error");
        assert_eq!(ErrorKind::UserAborted.as_str(), "User Aborted Error");
    }

    /// UT test cases for `HttpClientError` error kind function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::user_aborted`, `HttpClientError::other`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_kind() {
        let user_aborted = HttpClientError::user_aborted();
        assert_eq!(user_aborted.error_kind(), ErrorKind::UserAborted);
        let other = HttpClientError::other(user_aborted);
        assert_eq!(other.error_kind(), ErrorKind::Other);
    }

    /// UT test cases for `HttpClientError::from_io_error` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::from_io_error`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_from_io_error() {
        let error_build =
            HttpClientError::from_io_error(ErrorKind::Build, io::Error::from(io::ErrorKind::Other));
        assert_eq!(error_build.error_kind(), ErrorKind::Build);
    }

    /// UT test cases for `HttpClientError::from_error` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::from_error`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_from_error() {
        let error_build = HttpClientError::from_error(
            ErrorKind::Build,
            HttpClientError::from_str(ErrorKind::Request, "test error"),
        );
        assert_eq!(error_build.error_kind(), ErrorKind::Build);
    }

    /// UT test cases for `HttpClientError::from_str` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::from_str`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_from_str() {
        let error_request = HttpClientError::from_str(ErrorKind::Request, "error");
        assert_eq!(error_request.error_kind(), ErrorKind::Request);
        let error_timeout = HttpClientError::from_str(ErrorKind::Timeout, "error");
        assert_eq!(
            format!("{:?}", error_timeout),
            "HttpClientError { ErrorKind: Timeout, Cause: error }"
        );
        assert_eq!(format!("{error_timeout}"), "Timeout Error: error");
    }

    /// UT test cases for `HttpClientError::io_error` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::io_error`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_client_err_io_error() {
        let error = HttpClientError::from_io_error(
            ErrorKind::Request,
            io::Error::from(io::ErrorKind::BrokenPipe),
        );
        assert!(error.io_error().is_some());
        let error = HttpClientError::from_str(ErrorKind::Request, "error");
        assert!(error.io_error().is_none());
    }

    /// UT test cases for `Debug` of `HttpClientError`.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::fmt`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_client_err_debug_fmt() {
        let error = HttpClientError::from_io_error(
            ErrorKind::Request,
            io::Error::from(io::ErrorKind::BrokenPipe),
        );
        assert_eq!(
            format!("{:?}", error),
            "HttpClientError { ErrorKind: Request, Cause: Kind(BrokenPipe) }"
        );

        let error = HttpClientError::user_aborted();
        assert_eq!(
            format!("{:?}", error),
            "HttpClientError { ErrorKind: UserAborted, Cause: No reason }"
        );

        let error = HttpClientError::other(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(
            format!("{:?}", error),
            "HttpClientError { ErrorKind: Other, Cause: Kind(BrokenPipe) }"
        );
    }

    /// UT test cases for `Display` of `HttpClientError`.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::fmt`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_client_err_display_fmt() {
        let error = HttpClientError::from_io_error(
            ErrorKind::Request,
            io::Error::from(io::ErrorKind::BrokenPipe),
        );
        assert_eq!(format!("{}", error), "Request Error: broken pipe");

        let error = HttpClientError::user_aborted();
        assert_eq!(format!("{}", error), "User Aborted Error: No reason");

        let error = HttpClientError::other(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(format!("{}", error), "Other Error: broken pipe");
    }
}
