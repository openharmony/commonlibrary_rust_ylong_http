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
use std::error::Error;

/// The structure encapsulates errors that can be encountered when working with
/// the HTTP client.
pub struct HttpClientError {
    kind: ErrorKind,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl HttpClientError {
    /// Creates a `UserAborted` error.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::HttpClientError;
    ///
    /// let user_aborted = HttpClientError::user_aborted();
    /// ```
    pub fn user_aborted() -> Self {
        Self {
            kind: ErrorKind::UserAborted,
            cause: None,
        }
    }

    /// Creates an `Other` error.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::HttpClientError;
    ///
    /// # fn error(error: std::io::Error) {
    /// let other = HttpClientError::other(Some(error));
    /// # }
    /// ```
    pub fn other<T: Into<Box<dyn Error + Send + Sync>>>(cause: Option<T>) -> Self {
        Self {
            kind: ErrorKind::Other,
            cause: cause.map(|e| e.into()),
        }
    }

    /// Gets the `ErrorKind` of this `HttpClientError`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::{ErrorKind, HttpClientError};
    ///
    /// let user_aborted = HttpClientError::user_aborted();
    /// assert_eq!(user_aborted.error_kind(), ErrorKind::UserAborted);
    /// ```
    pub fn error_kind(&self) -> ErrorKind {
        self.kind
    }

    pub(crate) fn new_with_cause<T>(kind: ErrorKind, cause: Option<T>) -> Self
    where
        T: Into<Box<dyn Error + Send + Sync>>,
    {
        Self {
            kind,
            cause: cause.map(|e| e.into()),
        }
    }

    pub(crate) fn new_with_message(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            cause: Some(CauseMessage::new(message).into()),
        }
    }
}

impl Debug for HttpClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let mut builder = f.debug_struct("HttpClientError");
        builder.field("ErrorKind", &self.kind);
        if let Some(ref cause) = self.cause {
            builder.field("Cause", cause);
        }
        builder.finish()
    }
}

impl Display for HttpClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.kind.as_str())?;

        if let Some(ref cause) = self.cause {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}

impl Error for HttpClientError {}

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
    /// use ylong_http_client::ErrorKind;
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

/// Messages for summarizing the cause of the error
pub(crate) struct CauseMessage(String);

impl CauseMessage {
    pub(crate) fn new(message: &str) -> Self {
        Self(message.to_string())
    }
}

impl Debug for CauseMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl Display for CauseMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl Error for CauseMessage {}

#[cfg(test)]
mod ut_util_error {
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
        let other = HttpClientError::other(Some("error"));
        assert_eq!(other.error_kind(), ErrorKind::Other);
    }

    /// UT test cases for `HttpClientError::new_with_cause` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::new_with_cause`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_with_cause() {
        let error_build = HttpClientError::new_with_cause(ErrorKind::Build, Some("error"));
        assert_eq!(error_build.error_kind(), ErrorKind::Build);
    }

    /// UT test cases for `HttpClientError::new_with_message` function.
    ///
    /// # Brief
    /// 1. Calls `HttpClientError::new_with_message`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_err_with_message() {
        let error_request = HttpClientError::new_with_message(ErrorKind::Request, "error");
        assert_eq!(error_request.error_kind(), ErrorKind::Request);
        let error_timeout = HttpClientError::new_with_message(ErrorKind::Timeout, "error");
        assert_eq!(
            format!("{:?}", error_timeout),
            "HttpClientError { ErrorKind: Timeout, Cause: error }"
        );
        assert_eq!(format!("{error_timeout}"), "Timeout Error: error");
    }
}
