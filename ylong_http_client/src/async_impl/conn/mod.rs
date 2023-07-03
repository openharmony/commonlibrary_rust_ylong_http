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

use crate::async_impl::HttpBody;
use crate::error::HttpClientError;
use crate::util::dispatcher::Conn;
use crate::{AsyncRead, AsyncWrite};
use ylong_http::body::async_impl::Body;

use crate::async_impl::client::Retryable;
use ylong_http::request::Request;
use ylong_http::response::Response;

#[cfg(feature = "http1_1")]
mod http1;

#[cfg(feature = "http2")]
mod http2;

pub(crate) trait StreamData: AsyncRead {
    fn shutdown(&self);
}

pub(crate) async fn request<S, T>(
    conn: Conn<S>,
    request: &mut Request<T>,
    _retryable: &mut Retryable,
) -> Result<Response<HttpBody>, HttpClientError>
where
    T: Body,
    S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
{
    match conn {
        #[cfg(feature = "http1_1")]
        Conn::Http1(http1) => http1::request(http1, request).await,

        #[cfg(feature = "http2")]
        Conn::Http2(http2) => http2::request(http2, request, _retryable).await,
    }
}
