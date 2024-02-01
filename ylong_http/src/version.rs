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

//! HTTP [`Version`].
//!
//! HTTP's version number consists of two decimal digits separated by a "."
//! (period or decimal point). The first digit (major version) indicates the
//! messaging syntax, whereas the second digit (minor version) indicates the
//! highest minor version within that major version to which the sender is
//! conformant (able to understand for future communication).
//!
//! [`Version`]: https://httpwg.org/specs/rfc9110.html#protocol.version

use core::convert::TryFrom;

use crate::error::{ErrorKind, HttpError};

/// HTTP [`Version`] implementation.
///
/// [`Version`]: https://httpwg.org/specs/rfc9110.html#protocol.version
///
/// # Examples
///
/// ```
/// use ylong_http::version::Version;
///
/// assert_eq!(Version::HTTP1_1.as_str(), "HTTP/1.1");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version(Inner);

impl Version {
    /// HTTP/1.0
    pub const HTTP1_0: Self = Self(Inner::Http10);
    /// HTTP/1.1
    pub const HTTP1_1: Self = Self(Inner::Http11);
    /// HTTP/2
    pub const HTTP2: Self = Self(Inner::Http2);
    /// HTTP/3
    pub const HTTP3: Self = Self(Inner::Http3);

    /// Converts a `Version` to a `&str`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::version::Version;
    ///
    /// assert_eq!(Version::HTTP1_1.as_str(), "HTTP/1.1");
    /// ```
    pub fn as_str(&self) -> &str {
        match self.0 {
            Inner::Http10 => "HTTP/1.0",
            Inner::Http11 => "HTTP/1.1",
            Inner::Http2 => "HTTP/2.0",
            Inner::Http3 => "HTTP/3.0",
        }
    }
}

impl<'a> TryFrom<&'a str> for Version {
    type Error = HttpError;

    fn try_from(str: &'a str) -> Result<Self, Self::Error> {
        match str {
            "HTTP/1.0" => Ok(Version::HTTP1_0),
            "HTTP/1.1" => Ok(Version::HTTP1_1),
            "HTTP/2.0" => Ok(Version::HTTP2),
            "HTTP/3.0" => Ok(Version::HTTP3),
            _ => Err(ErrorKind::InvalidInput.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Inner {
    Http10,
    Http11,
    Http2,
    Http3,
}

#[cfg(test)]
mod ut_version {
    use std::convert::TryFrom;

    use super::Version;

    /// UT test cases for `Version::as_str`.
    ///
    /// # Brief
    /// 1. Checks whether `Version::as_str` is correct.
    #[test]
    fn ut_version_as_str() {
        assert_eq!(Version::HTTP1_0.as_str(), "HTTP/1.0");
        assert_eq!(Version::HTTP1_1.as_str(), "HTTP/1.1");
        assert_eq!(Version::HTTP2.as_str(), "HTTP/2.0");
        assert_eq!(Version::HTTP3.as_str(), "HTTP/3.0");
    }

    /// UT test cases for `Version::try_from`.
    ///
    /// # Brief
    /// 1. Checks whether `Version::try_from` is correct.
    #[test]
    fn ut_version_try_from() {
        assert_eq!(Version::try_from("HTTP/1.0").unwrap(), Version::HTTP1_0);
        assert_eq!(Version::try_from("HTTP/1.1").unwrap(), Version::HTTP1_1);
        assert_eq!(Version::try_from("HTTP/2.0").unwrap(), Version::HTTP2);
        assert_eq!(Version::try_from("HTTP/3.0").unwrap(), Version::HTTP3);
        assert!(Version::try_from("http/1.0").is_err());
        assert!(Version::try_from("http/1.1").is_err());
        assert!(Version::try_from("http/2.0").is_err());
        assert!(Version::try_from("http/3.0").is_err());
    }
}
