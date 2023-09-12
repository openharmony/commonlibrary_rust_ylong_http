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

//! HTTP synchronous client module.
//!
//! This module provides synchronous client components.
//!
//! - [`Client`]: The main part of client, which provides the request sending
//! interface and configuration interface. `Client` sends requests in a
//! synchronous blocking manner.
//!
//! - [`Connector`]: `Connector`s are used to create new connections
//! synchronously. This module provides `Connector` trait and a `HttpConnector`
//! which implements the trait.

// TODO: Reconstruct `sync_impl`, or reuse `async_impl`?

mod client;
mod conn;
mod connector;
mod http_body;
mod pool;
mod reader;

pub use client::{Client, ClientBuilder};
pub use connector::Connector;
pub(crate) use connector::HttpConnector;
pub use http_body::HttpBody;
pub use reader::{BodyProcessError, BodyProcessor, BodyReader, DefaultBodyProcessor};
pub use ylong_http::body::sync_impl::Body;
pub use ylong_http::body::{EmptyBody, TextBody};
pub use ylong_http::request::Request;
pub use ylong_http::response::Response;

#[cfg(feature = "__tls")]
mod ssl_stream;
#[cfg(feature = "__tls")]
pub use ssl_stream::MixStream;
