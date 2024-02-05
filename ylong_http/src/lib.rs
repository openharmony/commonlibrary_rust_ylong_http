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

#![allow(dead_code)]
#![allow(unused_imports)]

//! `ylong_http` provides various basic components that `HTTP` needs to use.
//! You can use these components to build a HTTP client, a HTTP server, etc.
//!
//! # Support HTTP Version
//! - `HTTP1.1`
// TODO: Need doc.

#[cfg(feature = "http1_1")]
pub mod h1;

#[cfg(feature = "http2")]
pub mod h2;

/// Module that contains the functionality for HTTP/3 support.
#[cfg(feature = "http3")]
pub mod h3;

#[cfg(feature = "huffman")]
mod huffman;

#[cfg(any(feature = "ylong_base", feature = "tokio_base"))]
pub mod body;
pub mod error;
pub mod headers;
pub mod request;
pub mod response;
pub mod version;

pub(crate) mod util;

#[cfg(feature = "tokio_base")]
pub(crate) use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};
#[cfg(feature = "ylong_base")]
pub(crate) use ylong_runtime::io::{AsyncRead, AsyncReadExt, ReadBuf};
