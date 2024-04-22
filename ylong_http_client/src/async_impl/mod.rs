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

//! HTTP asynchronous client module.
//!
//! This module provides asynchronous client components.
//!
//! - [`Client`]: The main part of client, which provides the request sending
//! interface and configuration interface. `Client` sends requests in an
//! asynchronous manner.
//!
//! - [`Connector`]: `Connector`s are used to create new connections
//! asynchronously. This module provides `Connector` trait and a `HttpConnector`
//! which implements the trait.

mod client;
mod connector;
mod downloader;
mod http_body;
mod interceptor;
mod request;
mod response;
mod timeout;
mod uploader;

#[cfg(feature = "__tls")]
mod ssl_stream;

pub(crate) mod conn;
pub(crate) mod pool;

pub use client::ClientBuilder;
pub use connector::{Connector, HttpConnector};
pub use downloader::{DownloadOperator, Downloader, DownloaderBuilder};
pub use http_body::HttpBody;
pub use interceptor::{ConnDetail, ConnProtocol, Interceptor};
pub use request::{Body, PercentEncoder, Request, RequestBuilder};
pub use response::Response;
pub use uploader::{UploadOperator, Uploader, UploaderBuilder};
pub use ylong_http::body::{MultiPart, Part};

// TODO: Remove these later.
/// Client Adapter.
pub type Client = client::Client<HttpConnector>;
