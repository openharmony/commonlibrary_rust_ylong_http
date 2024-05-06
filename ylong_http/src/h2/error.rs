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

//! [`Error Codes`] in [`HTTP/2`].
//!
//! [`Error Codes`]: https://httpwg.org/specs/rfc9113.html#ErrorCodes
//! [`HTTP/2`]: https://httpwg.org/specs/rfc9113.html
//!
//! # introduction
//! Error codes are 32-bit fields that are used in `RST_STREAM` and `GOAWAY`
//! frames to convey the reasons for the stream or connection error.
//!
//! Error codes share a common code space. Some error codes apply only to either
//! streams or the entire connection and have no defined semantics in the other
//! context.

use std::convert::{Infallible, TryFrom};

use crate::error::{ErrorKind, HttpError};

/// The http2 error handle implementation
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum H2Error {
    /// [`Stream Error`] Handling.
    ///
    /// [`Stream Error`]: https://www.rfc-editor.org/rfc/rfc9113.html#name-stream-error-handling
    StreamError(u32, ErrorCode),

    /// [`Connection Error`] Handling.
    ///
    /// [`Connection Error`]: https://www.rfc-editor.org/rfc/rfc9113.html#name-connection-error-handling
    ConnectionError(ErrorCode),
}

/// [`Error Codes`] implementation.
///
/// [`Error Codes`]: https://httpwg.org/specs/rfc9113.html#ErrorCodes
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ErrorCode {
    /// The associated condition is not a result of an error. For example,
    /// a `GOAWAY` might include this code to indicate graceful shutdown of a
    /// connection.
    NoError = 0x00,

    /// The endpoint detected an unspecific protocol error. This error is for
    /// use when a more specific error code is not available.
    ProtocolError = 0x01,

    /// The endpoint encountered an unexpected internal error.
    IntervalError = 0x02,

    /// The endpoint detected that its peer violated the flow-control protocol.
    FlowControlError = 0x03,

    /// The endpoint sent a `SETTINGS` frame but did not receive a response in
    /// a timely manner.
    SettingsTimeout = 0x04,

    /// The endpoint received a frame after a stream was half-closed.
    StreamClosed = 0x05,

    /// The endpoint received a frame with an invalid size.
    FrameSizeError = 0x06,

    /// The endpoint refused the stream prior to performing any application
    /// processing.
    RefusedStream = 0x07,

    /// The endpoint uses this error code to indicate that the stream is no
    /// longer needed.
    Cancel = 0x08,

    /// The endpoint is unable to maintain the field section compression context
    /// for the connection.
    CompressionError = 0x09,

    /// The connection established in response to a `CONNECT` request was reset
    /// or abnormally closed.
    ConnectError = 0x0a,

    /// The endpoint detected that its peer is exhibiting a behavior that might
    /// be generating excessive load.
    EnhanceYourCalm = 0x0b,

    /// The underlying transport has properties that do not meet minimum
    /// security requirements.
    InadequateSecurity = 0x0c,

    /// The endpoint requires that HTTP/1.1 be used instead of HTTP/2.
    Http1_1Required = 0x0d,
}

impl ErrorCode {
    /// Gets the error code of the `ErrorCode` enum.
    pub fn into_code(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for ErrorCode {
    type Error = H2Error;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let err = match value {
            0x00 => ErrorCode::NoError,
            0x01 => ErrorCode::ProtocolError,
            0x02 => ErrorCode::IntervalError,
            0x03 => ErrorCode::FlowControlError,
            0x04 => ErrorCode::SettingsTimeout,
            0x05 => ErrorCode::StreamClosed,
            0x06 => ErrorCode::FrameSizeError,
            0x07 => ErrorCode::RefusedStream,
            0x08 => ErrorCode::Cancel,
            0x09 => ErrorCode::CompressionError,
            0x0a => ErrorCode::ConnectError,
            0x0b => ErrorCode::EnhanceYourCalm,
            0x0c => ErrorCode::InadequateSecurity,
            0x0d => ErrorCode::Http1_1Required,
            _ => return Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
        };
        Ok(err)
    }
}

#[cfg(test)]
mod ut_h2_error {
    use std::convert::TryInto;

    use super::*;

    /// Unit test cases for `ErrorCode::try_from`.
    ///
    /// # Brief
    /// 1. Iterates over a range of valid u32 values that represent
    ///    `ErrorCode`s.
    /// 2. Attempts to convert each u32 value into an `ErrorCode` using
    ///    `try_into`.
    /// 3. Checks that the conversion is successful for each valid `ErrorCode`.
    /// 4. Also attempts to convert an invalid u32 value into an `ErrorCode`.
    /// 5. Checks that the conversion fails for the invalid value.
    #[test]
    fn ut_test_error_code_try_from() {
        // Test conversion from u32 to ErrorCode for valid error codes
        for i in 0x00..=0x0d {
            let error_code: Result<ErrorCode, _> = i.try_into();
            assert!(error_code.is_ok());
        }

        // Test conversion from u32 to ErrorCode for invalid error codes
        let invalid_error_code: Result<ErrorCode, _> = 0x0e.try_into();
        assert!(invalid_error_code.is_err());
    }
}
