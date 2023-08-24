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

//! `ylong_http_client` provides a HTTP client that based on `ylong_http` crate.
//! You can use the client to send request to a server, and then get the
//! response.
//!
//! # Supported HTTP Version
//! - HTTP1.1
// TODO: Need doc.

// ylong_http crate re-export.
pub use ylong_http::body::{EmptyBody, TextBody};
pub use ylong_http::request::method::Method;
pub use ylong_http::request::uri::{Scheme, Uri};
pub use ylong_http::request::{Request, RequestPart};
pub use ylong_http::response::status::StatusCode;
pub use ylong_http::response::ResponsePart;
pub use ylong_http::version::Version;

#[cfg(all(feature = "async", any(feature = "http1_1", feature = "http2")))]
pub mod async_impl;

#[cfg(all(feature = "async", any(feature = "http1_1", feature = "http2")))]
pub use async_impl::{Body, RequestBuilder, Response};

#[cfg(all(feature = "sync", any(feature = "http1_1", feature = "http2")))]
pub mod sync_impl;

#[cfg(all(
    any(feature = "async", feature = "sync"),
    any(feature = "http1_1", feature = "http2"),
))]
mod error;

#[cfg(all(
    any(feature = "async", feature = "sync"),
    any(feature = "http1_1", feature = "http2"),
))]
pub use error::{ErrorKind, HttpClientError};

#[cfg(all(
    any(feature = "async", feature = "sync"),
    any(feature = "http1_1", feature = "http2"),
))]
pub mod util;

#[cfg(all(feature = "tokio_base", feature = "http2"))]
pub(crate) use tokio::sync::{
    mpsc::{error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex as AsyncMutex, MutexGuard,
};
#[cfg(all(feature = "tokio_base", feature = "async"))]
pub(crate) use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
    time::{sleep, timeout, Sleep},
};
#[cfg(all(
    any(feature = "async", feature = "sync"),
    any(feature = "http1_1", feature = "http2"),
))]
pub use util::*;
#[cfg(all(feature = "ylong_base", feature = "http2"))]
pub(crate) use ylong_runtime::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex as AsyncMutex, MutexGuard, RecvError as TryRecvError,
};
#[cfg(all(feature = "ylong_base", feature = "async"))]
pub(crate) use ylong_runtime::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
    time::{sleep, timeout, Sleep},
};
