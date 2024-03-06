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

use core::pin::Pin;
use core::task::{Context, Poll};

use crate::async_impl::ssl_stream::AsyncSslStream;
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};

/// A stream which may be wrapped with TLS.
pub enum MixStream<T> {
    /// A raw HTTP stream.
    Http(T),
    /// An SSL-wrapped HTTP stream.
    Https(AsyncSslStream<T>),
}

impl<T> AsyncRead for MixStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // poll_read separately.
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_read(cx, buf),
            MixStream::Https(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<T> AsyncWrite for MixStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // poll_write separately.
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_write(ctx, buf),
            MixStream::Https(s) => Pin::new(s).poll_write(ctx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_flush(ctx),
            MixStream::Https(s) => Pin::new(s).poll_flush(ctx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_shutdown(ctx),
            MixStream::Https(s) => Pin::new(s).poll_shutdown(ctx),
        }
    }
}
