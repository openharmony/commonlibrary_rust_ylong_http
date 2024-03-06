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

//! HTTP [`Status Codes`].
//!
//! The status code of a response is a three-digit integer code that describes
//! the result of the request and the semantics of the response, including
//! whether the request was successful and what content is enclosed (if any).
//! All valid status codes are within the range of 100 to 599, inclusive.
//!
//! [`Status Codes`]: https://httpwg.org/specs/rfc9110.html#status.codes

use core::convert::TryFrom;
use core::fmt::{Display, Formatter};

use crate::error::{ErrorKind, HttpError};

/// HTTP [`Status Codes`] implementation.
///
/// [`Status Codes`]: https://httpwg.org/specs/rfc9110.html#status.codes
///
/// # Examples
///
/// ```
/// use ylong_http::response::status::StatusCode;
///
/// let status = StatusCode::OK;
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StatusCode(u16);

impl StatusCode {
    /// Converts a `u16` to a `StatusCode`.
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::from_u16(200), Ok(StatusCode::OK));
    /// ```
    pub fn from_u16(code: u16) -> Result<StatusCode, HttpError> {
        // Only three-digit status codes are valid.
        if !(100..1000).contains(&code) {
            return Err(ErrorKind::InvalidInput.into());
        }

        Ok(StatusCode(code))
    }

    /// Converts a `StatusCode` to a `u16`.
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::OK.as_u16(), 200u16);
    /// ```
    pub fn as_u16(&self) -> u16 {
        self.0
    }

    /// Converts a `&[u8]` to a `StatusCode`.
    ///
    /// # Example
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::from_bytes(b"200"), Ok(StatusCode::OK));
    /// assert!(StatusCode::from_bytes(b"0").is_err());
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HttpError> {
        // Only three-digit status codes are valid.
        if bytes.len() != 3 {
            return Err(ErrorKind::InvalidInput.into());
        }

        let a = bytes[0].wrapping_sub(b'0') as u16;
        let b = bytes[1].wrapping_sub(b'0') as u16;
        let c = bytes[2].wrapping_sub(b'0') as u16;

        if a == 0 || a > 9 || b > 9 || c > 9 {
            return Err(ErrorKind::InvalidInput.into());
        }

        // Valid status code: 1 <= a <= 9 && 0 <= b <= 9 && 0 <= c <= 9
        Ok(StatusCode((a * 100) + (b * 10) + c))
    }

    /// Converts a `StatusCode` to a `[u8; 3]`.
    ///
    /// # Example
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::OK.as_bytes(), *b"200");
    /// ```
    pub fn as_bytes(&self) -> [u8; 3] {
        [
            ((self.0 / 100) as u8) + b'0',
            (((self.0 % 100) / 10) as u8) + b'0',
            ((self.0 % 10) as u8) + b'0',
        ]
    }

    /// Determines whether the `StatusCode` is [`1xx (Informational)`].
    ///
    /// The 1xx (Informational) class of status code indicates an interim
    /// response for communicating connection status or request progress prior
    /// to completing the requested action and sending a final response.
    ///
    /// [`1xx (Informational)`]: https://httpwg.org/specs/rfc9110.html#status.1xx
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert!(StatusCode::CONTINUE.is_informational());
    /// assert!(!StatusCode::OK.is_informational());
    /// ```
    pub fn is_informational(&self) -> bool {
        self.0 >= 100 && 200 > self.0
    }

    /// Determines whether the `StatusCode` is [`2xx (Successful)`].
    ///
    /// The 2xx (Successful) class of status code indicates that the client's
    /// request was successfully received, understood, and accepted.
    ///
    /// [`2xx (Successful)`]: https://httpwg.org/specs/rfc9110.html#status.2xx
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert!(StatusCode::OK.is_successful());
    /// assert!(!StatusCode::CONTINUE.is_successful());
    /// ```
    pub fn is_successful(&self) -> bool {
        self.0 >= 200 && 300 > self.0
    }

    /// Determines whether the `StatusCode` is [`3xx (Redirection)`].
    ///
    /// The 3xx (Redirection) class of status code indicates that further action
    /// needs to be taken by the user agent in order to fulfill the request.
    ///
    /// [`3xx (Redirection)`]: https://httpwg.org/specs/rfc9110.html#status.3xx
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert!(StatusCode::MULTIPLE_CHOICES.is_redirection());
    /// assert!(!StatusCode::OK.is_redirection());
    /// ```
    pub fn is_redirection(&self) -> bool {
        self.0 >= 300 && 400 > self.0
    }

    /// Determines whether the `StatusCode` is [`4xx (Client Error)`].
    ///
    /// The 4xx (Client Error) class of status code indicates that the client
    /// seems to have erred.
    ///
    /// [`4xx (Client Error)`]: https://httpwg.org/specs/rfc9110.html#status.4xx
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert!(StatusCode::BAD_REQUEST.is_client_error());
    /// assert!(!StatusCode::OK.is_client_error());
    /// ```
    pub fn is_client_error(&self) -> bool {
        self.0 >= 400 && 500 > self.0
    }

    /// Determines whether the `StatusCode` is [`5xx (Server Error)`].
    ///
    /// The 5xx (Server Error) class of status code indicates that the server is
    /// aware that it has erred or is incapable of performing the requested
    /// method.
    ///
    /// [`5xx (Server Error)`]: https://httpwg.org/specs/rfc9110.html#status.5xx
    ///
    /// # Examples
    /// ```
    /// use ylong_http::response::status::StatusCode;
    ///
    /// assert!(StatusCode::INTERNAL_SERVER_ERROR.is_server_error());
    /// assert!(!StatusCode::OK.is_server_error());
    /// ```
    pub fn is_server_error(&self) -> bool {
        self.0 >= 500 && 600 > self.0
    }
}

impl TryFrom<u16> for StatusCode {
    type Error = HttpError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::from_u16(value)
    }
}

impl<'a> TryFrom<&'a [u8]> for StatusCode {
    type Error = HttpError;

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

macro_rules! status_list {
    (
        $(
            $(#[$docs: meta])*
            ($num:expr, $name: ident, $phrase: expr),
        )*
    ) => {
        impl StatusCode {
            $(
                $(#[$docs])*
                pub const $name: StatusCode = StatusCode($num as u16);
            )*

            /// Gets the reason of the `StatusCode`.
            ///
            /// # Examples
            ///
            /// ```
            /// use ylong_http::response::status::StatusCode;
            ///
            /// assert_eq!(StatusCode::OK.reason(), Some("OK"));
            /// ```
            pub fn reason(&self) -> Option<&'static str> {
                match self.0 {
                    $(
                        $num => Some($phrase),
                    )*
                    _ => None,
                }
            }
        }

        /// UT test cases for `StatusCode::reason`.
        ///
        /// # Brief
        /// 1. Creates all the valid `StatusCode`s.
        /// 2. Calls `StatusCode::reason` on them.
        /// 3. Checks if the results are correct.
        #[test]
        pub fn ut_status_code_reason() {
            $(
                assert_eq!(StatusCode::from_u16($num as u16).unwrap().reason(), Some($phrase));
            )*
            assert_eq!(StatusCode::from_u16(999).unwrap().reason(), None);
        }
    }
}

// TODO: Adapter, remove this later.
impl Display for StatusCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} {}",
            self.as_u16(),
            self.reason().unwrap_or("Unknown status code")
        )
    }
}

#[rustfmt::skip]
status_list!(
    /// [`100 Continue`]: https://tools.ietf.org/html/rfc7231#section-6.2.1
    (100, CONTINUE, "Continue"),

    /// [`101 Switching Protocols`]: https://tools.ietf.org/html/rfc7231#section-6.2.2
    (101, SWITCHING_PROTOCOLS, "Switching Protocols"),

    /// [`102 Processing`]: https://tools.ietf.org/html/rfc2518
    (102, PROCESSING, "Processing"),

    /// [`200 OK`]: https://tools.ietf.org/html/rfc7231#section-6.3.1
    (200, OK, "OK"),

    /// [`201 Created`]: https://tools.ietf.org/html/rfc7231#section-6.3.2
    (201, CREATED, "Created"),

    /// [`202 Accepted`]: https://tools.ietf.org/html/rfc7231#section-6.3.3
    (202, ACCEPTED, "Accepted"),

    /// [`203 Non-Authoritative Information`]: https://tools.ietf.org/html/rfc7231#section-6.3.4
    (203, NON_AUTHORITATIVE_INFORMATION, "Non Authoritative Information"),

    /// [`204 No Content`]: https://tools.ietf.org/html/rfc7231#section-6.3.5
    (204, NO_CONTENT, "No Content"),

    /// [`205 Reset Content`]: https://tools.ietf.org/html/rfc7231#section-6.3.6
    (205, RESET_CONTENT, "Reset Content"),

    /// [`206 Partial Content`]: https://tools.ietf.org/html/rfc7233#section-4.1
    (206, PARTIAL_CONTENT, "Partial Content"),

    /// [`207 Multi-Status`]: https://tools.ietf.org/html/rfc4918
    (207, MULTI_STATUS, "Multi-Status"),

    /// [`208 Already Reported`]: https://tools.ietf.org/html/rfc5842
    (208, ALREADY_REPORTED, "Already Reported"),

    /// [`226 IM Used`]: https://tools.ietf.org/html/rfc3229
    (226, IM_USED, "IM Used"),

    /// [`300 Multiple Choices`]: https://tools.ietf.org/html/rfc7231#section-6.4.1
    (300, MULTIPLE_CHOICES, "Multiple Choices"),

    /// [`301 Moved Permanently`]: https://tools.ietf.org/html/rfc7231#section-6.4.2
    (301, MOVED_PERMANENTLY, "Moved Permanently"),

    /// [`302 Found`]: https://tools.ietf.org/html/rfc7231#section-6.4.3
    (302, FOUND, "Found"),

    /// [`303 See Other`]: https://tools.ietf.org/html/rfc7231#section-6.4.4
    (303, SEE_OTHER, "See Other"),

    /// [`304 Not Modified`]: https://tools.ietf.org/html/rfc7232#section-4.1
    (304, NOT_MODIFIED, "Not Modified"),

    /// [`305 Use Proxy`]: https://tools.ietf.org/html/rfc7231#section-6.4.5
    (305, USE_PROXY, "Use Proxy"),

    /// [`307 Temporary Redirect`]: https://tools.ietf.org/html/rfc7231#section-6.4.7
    (307, TEMPORARY_REDIRECT, "Temporary Redirect"),

    /// [`308 Permanent Redirect`]: https://tools.ietf.org/html/rfc7238
    (308, PERMANENT_REDIRECT, "Permanent Redirect"),

    /// [`400 Bad Request`]: https://tools.ietf.org/html/rfc7231#section-6.5.1
    (400, BAD_REQUEST, "Bad Request"),

    /// [`401 Unauthorized`]: https://tools.ietf.org/html/rfc7235#section-3.1
    (401, UNAUTHORIZED, "Unauthorized"),

    /// [`402 Payment Required`]: https://tools.ietf.org/html/rfc7231#section-6.5.2
    (402, PAYMENT_REQUIRED, "Payment Required"),

    /// [`403 Forbidden`]: https://tools.ietf.org/html/rfc7231#section-6.5.3
    (403, FORBIDDEN, "Forbidden"),

    /// [`404 Not Found`]: https://tools.ietf.org/html/rfc7231#section-6.5.4
    (404, NOT_FOUND, "Not Found"),

    /// [`405 Method Not Allowed`]: https://tools.ietf.org/html/rfc7231#section-6.5.5
    (405, METHOD_NOT_ALLOWED, "Method Not Allowed"),

    /// [`406 Not Acceptable`]: https://tools.ietf.org/html/rfc7231#section-6.5.6
    (406, NOT_ACCEPTABLE, "Not Acceptable"),

    /// [`407 Proxy Authentication Required`]: https://tools.ietf.org/html/rfc7235#section-3.2
    (407, PROXY_AUTHENTICATION_REQUIRED, "Proxy Authentication Required"),

    /// [`408 Request Timeout`]: https://tools.ietf.org/html/rfc7231#section-6.5.7
    (408, REQUEST_TIMEOUT, "Request Timeout"),

    /// [`409 Conflict`]: https://tools.ietf.org/html/rfc7231#section-6.5.8
    (409, CONFLICT, "Conflict"),

    /// [`410 Gone`]: https://tools.ietf.org/html/rfc7231#section-6.5.9
    (410, GONE, "Gone"),

    /// [`411 Length Required`]: https://tools.ietf.org/html/rfc7231#section-6.5.10
    (411, LENGTH_REQUIRED, "Length Required"),

    /// [`412 Precondition Failed`]: https://tools.ietf.org/html/rfc7232#section-4.2
    (412, PRECONDITION_FAILED, "Precondition Failed"),

    /// [`413 Payload Too Large`]: https://tools.ietf.org/html/rfc7231#section-6.5.11
    (413, PAYLOAD_TOO_LARGE, "Payload Too Large"),

    /// [`414 URI Too Long`]: https://tools.ietf.org/html/rfc7231#section-6.5.12
    (414, URI_TOO_LONG, "URI Too Long"),

    /// [`415 Unsupported Media Type`]: https://tools.ietf.org/html/rfc7231#section-6.5.13
    (415, UNSUPPORTED_MEDIA_TYPE, "Unsupported Media Type"),

    /// [`416 Range Not Satisfiable`]: https://tools.ietf.org/html/rfc7233#section-4.4
    (416, RANGE_NOT_SATISFIABLE, "Range Not Satisfiable"),

    /// [`417 Expectation Failed`]: https://tools.ietf.org/html/rfc7231#section-6.5.14
    (417, EXPECTATION_FAILED, "Expectation Failed"),

    /// [`418 I'm a teapot`]: https://tools.ietf.org/html/rfc2324
    (418, IM_A_TEAPOT, "I'm a teapot"),

    /// [`421 Misdirected Request`]: http://tools.ietf.org/html/rfc7540#section-9.1.2
    (421, MISDIRECTED_REQUEST, "Misdirected Request"),

    /// [`422 Unprocessable Entity`]: https://tools.ietf.org/html/rfc4918
    (422, UNPROCESSABLE_ENTITY, "Unprocessable Entity"),

    /// [`423 Locked`]: https://tools.ietf.org/html/rfc4918
    (423, LOCKED, "Locked"),

    /// [`424 Failed Dependency`]: https://tools.ietf.org/html/rfc4918
    (424, FAILED_DEPENDENCY, "Failed Dependency"),

    /// [`426 Upgrade Required`]: https://tools.ietf.org/html/rfc7231#section-6.5.15
    (426, UPGRADE_REQUIRED, "Upgrade Required"),

    /// [`428 Precondition Required`]: https://tools.ietf.org/html/rfc6585
    (428, PRECONDITION_REQUIRED, "Precondition Required"),

    /// [`429 Too Many Requests`]: https://tools.ietf.org/html/rfc6585
    (429, TOO_MANY_REQUESTS, "Too Many Requests"),

    /// [`431 Request Header Fields Too Large`]: https://tools.ietf.org/html/rfc6585
    (431, REQUEST_HEADER_FIELDS_TOO_LARGE, "Request Header Fields Too Large"),

    /// [`451 Unavailable For Legal Reasons`]: http://tools.ietf.org/html/rfc7725
    (451, UNAVAILABLE_FOR_LEGAL_REASONS, "Unavailable For Legal Reasons"),

    /// [`500 Internal Server Error`]: https://tools.ietf.org/html/rfc7231#section-6.6.1
    (500, INTERNAL_SERVER_ERROR, "Internal Server Error"),

    /// [`501 Not Implemented`]: https://tools.ietf.org/html/rfc7231#section-6.6.2
    (501, NOT_IMPLEMENTED, "Not Implemented"),

    /// [`502 Bad Gateway`]: https://tools.ietf.org/html/rfc7231#section-6.6.3
    (502, BAD_GATEWAY, "Bad Gateway"),

    /// [`503 Service Unavailable`]: https://tools.ietf.org/html/rfc7231#section-6.6.4
    (503, SERVICE_UNAVAILABLE, "Service Unavailable"),

    /// [`504 Gateway Timeout`]: https://tools.ietf.org/html/rfc7231#section-6.6.5
    (504, GATEWAY_TIMEOUT, "Gateway Timeout"),

    /// [`505 HTTP Version Not Supported`]: https://tools.ietf.org/html/rfc7231#section-6.6.6
    (505, HTTP_VERSION_NOT_SUPPORTED, "HTTP Version Not Supported"),

    /// [`506 Variant Also Negotiates`]: https://tools.ietf.org/html/rfc2295
    (506, VARIANT_ALSO_NEGOTIATES, "Variant Also Negotiates"),

    /// [`507 Insufficient Storage`]: https://tools.ietf.org/html/rfc4918
    (507, INSUFFICIENT_STORAGE, "Insufficient Storage"),

    /// [`508 Loop Detected`]: https://tools.ietf.org/html/rfc5842
    (508, LOOP_DETECTED, "Loop Detected"),

    /// [`510 Not Extended`]: https://tools.ietf.org/html/rfc2774
    (510, NOT_EXTENDED, "Not Extended"),

    /// [`511 Network Authentication Required`]: https://tools.ietf.org/html/rfc6585
    (511, NETWORK_AUTHENTICATION_REQUIRED, "Network Authentication Required"),
);

#[cfg(test)]
mod ut_status_code {
    use super::StatusCode;
    use crate::error::{ErrorKind, HttpError};

    /// UT test cases for `StatusCode::from_bytes`.
    ///
    /// # Brief
    /// 1. Calls `StatusCode::from_bytes` with various inputs.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_status_code_from_u16() {
        // Normal Test Cases:
        assert_eq!(StatusCode::from_u16(100), Ok(StatusCode::CONTINUE));
        assert!(StatusCode::from_u16(999).is_ok());

        // Exception Test Cases:
        // 1. The given number is not in the range of 100 to 1000.
        assert_eq!(
            StatusCode::from_u16(0),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_u16(u16::MAX),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }

    /// UT test cases for `StatusCode::as_u16`.
    ///
    /// # Brief
    /// 1. Creates a `StatusCode`.
    /// 2. Calls `StatusCode::as_u16` on it.
    /// 3. Checks if the result is correct.
    #[test]
    fn ut_status_code_as_u16() {
        assert_eq!(StatusCode::OK.as_u16(), 200);
    }

    /// UT test cases for `StatusCode::from_bytes`.
    ///
    /// # Brief
    /// 1. Calls `StatusCode::from_bytes` with various inputs.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_status_code_from_bytes() {
        // Normal Test Cases:
        assert_eq!(StatusCode::from_bytes(b"100"), Ok(StatusCode::CONTINUE));
        assert_eq!(
            StatusCode::from_bytes(b"500"),
            Ok(StatusCode::INTERNAL_SERVER_ERROR)
        );
        assert!(StatusCode::from_bytes(b"999").is_ok());

        // Exception Test Cases:
        // 1. Empty bytes slice.
        assert_eq!(
            StatusCode::from_bytes(b""),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 2. The length of the bytes slice is not 3.
        assert_eq!(
            StatusCode::from_bytes(b"1"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_bytes(b"1000"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );

        // 3. Other error branch coverage test cases.
        assert_eq!(
            StatusCode::from_bytes(b"099"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_bytes(b"a99"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_bytes(b"1a9"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_bytes(b"19a"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
        assert_eq!(
            StatusCode::from_bytes(b"\n\n\n"),
            Err(HttpError::from(ErrorKind::InvalidInput))
        );
    }

    /// UT test cases for `StatusCode::is_informational`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::is_informational` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_is_informational() {
        assert!(StatusCode::CONTINUE.is_informational());
        assert!(!StatusCode::OK.is_informational());
        assert!(!StatusCode::MULTIPLE_CHOICES.is_informational());
        assert!(!StatusCode::BAD_REQUEST.is_informational());
        assert!(!StatusCode::INTERNAL_SERVER_ERROR.is_informational());
        assert!(!StatusCode::from_u16(999).unwrap().is_informational());
    }

    /// UT test cases for `StatusCode::is_successful`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::is_successful` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_is_successful() {
        assert!(!StatusCode::CONTINUE.is_successful());
        assert!(StatusCode::OK.is_successful());
        assert!(!StatusCode::MULTIPLE_CHOICES.is_successful());
        assert!(!StatusCode::BAD_REQUEST.is_successful());
        assert!(!StatusCode::INTERNAL_SERVER_ERROR.is_successful());
        assert!(!StatusCode::from_u16(999).unwrap().is_successful());
    }

    /// UT test cases for `StatusCode::is_redirection`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::is_redirection` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_is_redirection() {
        assert!(!StatusCode::CONTINUE.is_redirection());
        assert!(!StatusCode::OK.is_redirection());
        assert!(StatusCode::MULTIPLE_CHOICES.is_redirection());
        assert!(!StatusCode::BAD_REQUEST.is_redirection());
        assert!(!StatusCode::INTERNAL_SERVER_ERROR.is_redirection());
        assert!(!StatusCode::from_u16(999).unwrap().is_redirection());
    }

    /// UT test cases for `StatusCode::is_client_error`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::is_client_error` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_is_client_error() {
        assert!(!StatusCode::CONTINUE.is_client_error());
        assert!(!StatusCode::OK.is_client_error());
        assert!(!StatusCode::MULTIPLE_CHOICES.is_client_error());
        assert!(StatusCode::BAD_REQUEST.is_client_error());
        assert!(!StatusCode::INTERNAL_SERVER_ERROR.is_client_error());
        assert!(!StatusCode::from_u16(999).unwrap().is_client_error());
    }

    /// UT test cases for `StatusCode::is_server_error`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::is_server_error` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_is_server_error() {
        assert!(!StatusCode::CONTINUE.is_server_error());
        assert!(!StatusCode::OK.is_server_error());
        assert!(!StatusCode::MULTIPLE_CHOICES.is_server_error());
        assert!(!StatusCode::BAD_REQUEST.is_server_error());
        assert!(StatusCode::INTERNAL_SERVER_ERROR.is_server_error());
        assert!(!StatusCode::from_u16(999).unwrap().is_server_error());
    }

    /// UT test cases for `StatusCode::as_bytes`.
    ///
    /// # Brief
    /// 1. Creates some `StatusCode`s that have different type with each other.
    /// 2. Calls `StatusCode::as_bytes` on them.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_status_code_as_bytes() {
        assert_eq!(StatusCode::OK.as_bytes(), *b"200");
        assert_eq!(StatusCode::FOUND.as_bytes(), *b"302");
        assert_eq!(StatusCode::NOT_FOUND.as_bytes(), *b"404");
        assert_eq!(StatusCode::GATEWAY_TIMEOUT.as_bytes(), *b"504");
    }
}
