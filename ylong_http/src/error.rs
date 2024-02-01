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

//! Errors that may occur in this crate.
//!
//! This module provide unified encapsulation of HTTP errors.
//!
//! [`HttpError`] encapsulates error information related to all http protocols
//! including `UriError`, `H1Error`, etc.
//!
//! [`HttpError`]: HttpError

use core::fmt::{Debug, Display, Formatter};
use std::convert::Infallible;
use std::error::Error;

#[cfg(feature = "http1_1")]
use crate::h1::H1Error;
#[cfg(feature = "http2")]
use crate::h2::H2Error;
use crate::request::uri::InvalidUri;

/// Errors that may occur when using this crate.
#[derive(Debug, Eq, PartialEq)]
pub struct HttpError {
    kind: ErrorKind,
}

impl From<ErrorKind> for HttpError {
    fn from(kind: ErrorKind) -> Self {
        HttpError { kind }
    }
}

impl From<InvalidUri> for HttpError {
    fn from(err: InvalidUri) -> Self {
        ErrorKind::Uri(err).into()
    }
}

#[cfg(feature = "http2")]
impl From<H2Error> for HttpError {
    fn from(err: H2Error) -> Self {
        ErrorKind::H2(err).into()
    }
}

impl From<Infallible> for HttpError {
    fn from(_value: Infallible) -> Self {
        unreachable!()
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Error for HttpError {}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ErrorKind {
    /// An invalid input parameter was passed to a method of this crate.
    InvalidInput,

    /// Errors related to URIs.
    Uri(InvalidUri),

    /// Errors related to `HTTP/1`.
    #[cfg(feature = "http1_1")]
    H1(H1Error),

    /// Errors related to `HTTP/2`.
    #[cfg(feature = "http2")]
    H2(H2Error),
}
