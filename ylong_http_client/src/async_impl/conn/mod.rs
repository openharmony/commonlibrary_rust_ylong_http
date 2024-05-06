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

#[cfg(feature = "http1_1")]
mod http1;

#[cfg(feature = "http2")]
mod http2;

use crate::async_impl::connector::ConnInfo;
use crate::async_impl::request::Message;
use crate::async_impl::Response;
use crate::error::HttpClientError;
use crate::runtime::{AsyncRead, AsyncWrite};
use crate::util::dispatcher::Conn;

pub(crate) trait StreamData: AsyncRead {
    fn shutdown(&self);
}

// TODO: Use structures instead of a function to reuse the io buf.
// TODO: Maybe `AsyncWrapper<Conn<S>>` ?.

pub(crate) async fn request<S>(conn: Conn<S>, message: Message) -> Result<Response, HttpClientError>
where
    S: AsyncRead + AsyncWrite + ConnInfo + Sync + Send + Unpin + 'static,
{
    match conn {
        #[cfg(feature = "http1_1")]
        Conn::Http1(http1) => http1::request(http1, message).await,

        #[cfg(feature = "http2")]
        Conn::Http2(http2) => http2::request(http2, message).await,
    }
}
