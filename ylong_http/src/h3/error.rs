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

use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

use crate::h3::qpack::error::QpackError;

/// HTTP3 errors.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum H3Error {
    /// Serialization error.
    Encode(EncodeError),
    /// Deserialization error.
    Decode(DecodeError),
    /// Common error during serialization or deserialization.
    Serialize(CommonError),
    /// Connection level error.
    Connection(H3ErrorCode),
    /// Stream level error.
    Stream(u64, H3ErrorCode),
}

/// Error during serialization.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum EncodeError {
    /// The set frame could not be found during serialization.
    NoCurrentFrame,
    /// The type of frame set does not match the serialized one.
    WrongTypeFrame,
    /// The previous frame has not been serialized.
    RepeatSetFrame,
    /// Sets a frame of unknown type.
    UnknownFrameType,
    /// Too many additional Settings are encoded.
    TooManySettings,
    /// qpack encoder encoding error.
    QpackError(QpackError),
}

/// Error during deserialization.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum DecodeError {
    /// The frame type does not correspond to the stream type.
    UnexpectedFrame(u64),
    /// Qpack decoder decoding error.
    QpackError(QpackError),
    /// The payload length resolved is different from the actual data.
    FrameSizeError(u64),
    /// Http3 does not allow the type of setting.
    UnsupportedSetting(u64),
}

/// Errors during serialization and deserialization,
/// usually occur during variable interger serialization and deserialization.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum CommonError {
    /// The buf used to store serialized data is too short.
    BufferTooShort,
    /// The field for the frame is missing.
    FieldMissing,
    /// Computation time overflow.
    CalculateOverflow,
    /// Internal error.
    InternalError,
}

/// Common http3 error codes defined in the rfc documentation.
/// Refers to [`iana`].
///
/// [`iana`]: https://www.iana.org/assignments/http3-parameters/http3-parameters.xhtml#http3-parameters-error-codes
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum H3ErrorCode {
    /// Datagram or Capsule Protocol parse error.
    H3DatagramError = 0x33,
    /// No error.
    H3NoError = 0x100,
    /// General protocol error.
    H3GeneralProtocolError = 0x101,
    /// Internal error.
    H3InternalError = 0x102,
    /// Stream creation error.
    H3StreamCreationError = 0x103,
    /// Critical stream was closed.
    H3ClosedCriticalStream = 0x104,
    /// Frame not permitted in the current state.
    H3FrameUnexpected = 0x105,
    /// Frame violated layout or size rules.
    H3FrameError = 0x106,
    /// Peer generating excessive load.
    H3ExcessiveLoad = 0x107,
    /// An identifier was used incorrectly.
    H3IdError = 0x108,
    /// SETTINGS frame contained invalid values.
    H3SettingsError = 0x109,
    /// No SETTINGS frame received.
    H3MissingSettings = 0x10A,
    /// Request not processed.
    H3RequestRejected = 0x10B,
    /// Data no longer needed.
    H3RequestCancelled = 0x10C,
    /// Stream terminated early.
    H3RequestIncomplete = 0x10D,
    /// Malformed message.
    H3MessageError = 0x10E,
    /// TCP reset or error on CONNECT request.
    H3ConnectError = 0x10F,
    /// Retry over HTTP/1.1.
    H3VersionFallback = 0x110,
    /// Decoding of a field section failed.
    QPACKDecompressionFailed = 0x200,
    /// Error on the encoder stream.
    QPACKEncoderStreamError = 0x201,
    /// Error on the decoder stream.
    QPACKDecoderStreamError = 0x202,
}

impl From<u64> for H3ErrorCode {
    fn from(value: u64) -> Self {
        match value {
            0x33 => H3ErrorCode::H3DatagramError,
            0x100 => H3ErrorCode::H3NoError,
            0x101 => H3ErrorCode::H3GeneralProtocolError,
            0x102 => H3ErrorCode::H3InternalError,
            0x103 => H3ErrorCode::H3StreamCreationError,
            0x104 => H3ErrorCode::H3ClosedCriticalStream,
            0x105 => H3ErrorCode::H3FrameUnexpected,
            0x106 => H3ErrorCode::H3FrameError,
            0x107 => H3ErrorCode::H3ExcessiveLoad,
            0x108 => H3ErrorCode::H3IdError,
            0x109 => H3ErrorCode::H3SettingsError,
            0x10A => H3ErrorCode::H3MissingSettings,
            0x10B => H3ErrorCode::H3RequestRejected,
            0x10C => H3ErrorCode::H3RequestCancelled,
            0x10D => H3ErrorCode::H3RequestIncomplete,
            0x10E => H3ErrorCode::H3MessageError,
            0x10F => H3ErrorCode::H3ConnectError,
            0x110 => H3ErrorCode::H3VersionFallback,
            0x200 => H3ErrorCode::QPACKDecompressionFailed,
            0x201 => H3ErrorCode::QPACKEncoderStreamError,
            0x202 => H3ErrorCode::QPACKDecoderStreamError,
            _ => H3ErrorCode::H3GeneralProtocolError,
        }
    }
}

impl From<QpackError> for DecodeError {
    fn from(value: QpackError) -> Self {
        DecodeError::QpackError(value)
    }
}

impl From<QpackError> for EncodeError {
    fn from(value: QpackError) -> Self {
        EncodeError::QpackError(value)
    }
}

impl From<EncodeError> for H3Error {
    fn from(value: EncodeError) -> Self {
        H3Error::Encode(value)
    }
}

impl From<DecodeError> for H3Error {
    fn from(value: DecodeError) -> Self {
        H3Error::Decode(value)
    }
}

impl From<CommonError> for H3Error {
    fn from(value: CommonError) -> Self {
        H3Error::Serialize(value)
    }
}

impl From<Infallible> for H3Error {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

impl Display for H3Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Error for H3Error {}
