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

//! HTTP [`Method`].
//!
//! The request method token is the primary source of request semantics;
//! it indicates the purpose for which the client has made this request and what
//! is expected by the client as a successful result.
//!
//! [`Method`]: https://httpwg.org/specs/rfc9110.html#methods
//!
//! # Examples
//!
//! ```
//! use ylong_http::request::method::Method;
//!
//! assert_eq!(Method::GET.as_str(), "GET");
//! ```

use core::convert::TryFrom;

use crate::error::{ErrorKind, HttpError};

/// HTTP `Method` implementation.
///
/// # Examples
///
/// ```
/// use ylong_http::request::method::Method;
///
/// assert_eq!(Method::GET.as_str(), "GET");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Method(Inner);

impl Method {
    /// Transfer a current representation of the target resource.
    ///
    /// See [`RFC9110 9.3.1`] for more details.
    ///
    /// [`RFC9110 9.3.1`]: (https://httpwg.org/specs/rfc9110.html#GET)
    pub const GET: Self = Self(Inner::Get);

    /// Same as `GET`, but do not transfer the response content.
    ///
    /// See [`RFC9110 9.3.2`] for more details.
    ///
    /// [`RFC9110 9.3.2`]: https://httpwg.org/specs/rfc9110.html#HEAD
    pub const HEAD: Self = Self(Inner::Head);

    /// Perform resource-specific processing on the request content.
    ///
    /// See [`RFC9110 9.3.3`] for more details.
    ///
    /// [`RFC9110 9.3.3`]: https://httpwg.org/specs/rfc9110.html#POST
    pub const POST: Self = Self(Inner::Post);

    /// Replace all current representations of the target resource with the
    /// request content.
    ///
    /// See [`RFC9110 9.3.4`] for more details.
    ///
    /// [`RFC9110 9.3.4`]: https://httpwg.org/specs/rfc9110.html#PUT
    pub const PUT: Self = Self(Inner::Put);

    /// Remove all current representations of the target resource.
    ///
    /// See [`RFC9110 9.3.5`] for more details.
    ///
    /// [`RFC9110 9.3.5`]: https://httpwg.org/specs/rfc9110.html#DELETE
    pub const DELETE: Self = Self(Inner::Delete);

    /// Establish a tunnel to the server identified by the target resource.
    ///
    /// See [`RFC9110 9.3.6`] for more details.
    ///
    /// [`RFC9110 9.3.6`]: https://httpwg.org/specs/rfc9110.html#CONNECT
    pub const CONNECT: Self = Self(Inner::Connect);

    /// Describe the communication options for the target resource.
    ///
    /// See [`RFC9110 9.3.7`] for more details.
    ///
    /// [`RFC9110 9.3.7`]: https://httpwg.org/specs/rfc9110.html#OPTIONS
    pub const OPTIONS: Self = Self(Inner::Options);

    /// Perform a message loop-back test along the path to the target resource.
    ///
    /// See [`RFC9110 9.3.8`] for more details.
    ///
    /// [`RFC9110 9.3.8`]: https://httpwg.org/specs/rfc9110.html#TRACE
    pub const TRACE: Self = Self(Inner::Trace);

    /// Tries converting &[u8] to `Method`. Only uppercase letters are
    /// supported.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::method::Method;
    ///
    /// let method = Method::from_bytes(b"GET").unwrap();
    /// assert_eq!(method.as_str(), "GET");
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Method, HttpError> {
        if bytes.len() < 3 || bytes.len() > 7 {
            return Err(ErrorKind::InvalidInput.into());
        }
        match bytes[0] {
            b'G' if b"ET" == &bytes[1..] => Ok(Method::GET),
            b'P' => match bytes[1] {
                b'U' if b"T" == &bytes[2..] => Ok(Method::PUT),
                b'O' if b"ST" == &bytes[2..] => Ok(Method::POST),
                _ => Err(ErrorKind::InvalidInput.into()),
            },
            b'H' if b"EAD" == &bytes[1..] => Ok(Method::HEAD),
            b'T' if b"RACE" == &bytes[1..] => Ok(Method::TRACE),
            b'D' if b"ELETE" == &bytes[1..] => Ok(Method::DELETE),
            b'O' if b"PTIONS" == &bytes[1..] => Ok(Method::OPTIONS),
            b'C' if b"ONNECT" == &bytes[1..] => Ok(Method::CONNECT),
            _ => Err(ErrorKind::InvalidInput.into()),
        }
    }

    /// Converts `Method` to `&str` in uppercase.
    ///
    /// # Examples
    /// ```
    /// use ylong_http::request::method::Method;
    ///
    /// assert_eq!(Method::GET.as_str(), "GET");
    /// ```
    pub fn as_str(&self) -> &str {
        match self.0 {
            Inner::Get => "GET",
            Inner::Head => "HEAD",
            Inner::Post => "POST",
            Inner::Put => "PUT",
            Inner::Delete => "DELETE",
            Inner::Options => "OPTIONS",
            Inner::Trace => "TRACE",
            Inner::Connect => "CONNECT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Inner {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
}

impl<'a> TryFrom<&'a [u8]> for Method {
    type Error = HttpError;

    fn try_from(t: &'a [u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(t)
    }
}

impl<'a> TryFrom<&'a str> for Method {
    type Error = HttpError;

    fn try_from(t: &'a str) -> Result<Self, Self::Error> {
        Self::from_bytes(t.as_bytes())
    }
}

#[cfg(test)]
mod ut_method {
    use super::Method;
    use crate::error::{ErrorKind, HttpError};

    /// UT test cases for `Method::as_str`.
    ///
    /// # Brief
    /// 1. Calls `as_str` for all method kinds.
    /// 2. Checks the results.
    #[test]
    fn ut_method_as_str() {
        assert_eq!(Method::GET.as_str(), "GET");
        assert_eq!(Method::HEAD.as_str(), "HEAD");
        assert_eq!(Method::POST.as_str(), "POST");
        assert_eq!(Method::PUT.as_str(), "PUT");
        assert_eq!(Method::DELETE.as_str(), "DELETE");
        assert_eq!(Method::OPTIONS.as_str(), "OPTIONS");
        assert_eq!(Method::TRACE.as_str(), "TRACE");
        assert_eq!(Method::CONNECT.as_str(), "CONNECT");
    }

    /// UT test cases for `Method::from_bytes`.
    ///
    /// # Brief
    /// 1. Calls `from_bytes` and pass in various types of parameters.
    /// 2. Checks the results.
    #[test]
    fn ut_method_from_bytes() {
        // Normal Test Cases:
        assert_eq!(Method::from_bytes(b"GET").unwrap(), Method::GET);
        assert_eq!(Method::from_bytes(b"HEAD").unwrap(), Method::HEAD);
        assert_eq!(Method::from_bytes(b"POST").unwrap(), Method::POST);
        assert_eq!(Method::from_bytes(b"PUT").unwrap(), Method::PUT);
        assert_eq!(Method::from_bytes(b"DELETE").unwrap(), Method::DELETE);
        assert_eq!(Method::from_bytes(b"OPTIONS").unwrap(), Method::OPTIONS);
        assert_eq!(Method::from_bytes(b"TRACE").unwrap(), Method::TRACE);
        assert_eq!(Method::from_bytes(b"CONNECT").unwrap(), Method::CONNECT);

        // Exception Test Cases:
        // 1. Empty bytes slice.
        assert_eq!(
            Method::from_bytes(b""),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 2. The length of bytes slice is less than 3.
        assert_eq!(
            Method::from_bytes(b"G"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 3. The length of bytes slice is more than 7.
        assert_eq!(
            Method::from_bytes(b"CONNECTT"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 4. Mixed case letters inside the bytes slice.
        assert_eq!(
            Method::from_bytes(b"Get"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 5. Other error branch coverage test cases.
        assert_eq!(
            Method::from_bytes(b"PATC"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            Method::from_bytes(b"PATCHH"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            Method::from_bytes(b"ABCDEFG"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }
}
