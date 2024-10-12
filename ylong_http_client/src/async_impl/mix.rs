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

#[cfg(feature = "http3")]
use ylong_runtime::net::ConnectedUdpSocket;

use crate::async_impl::ssl_stream::AsyncSslStream;
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf, TcpStream};

/// A stream which may be wrapped with TLS.
pub enum MixStream {
    /// A raw HTTP stream.
    Http(TcpStream),
    /// An SSL-wrapped HTTP stream.
    Https(AsyncSslStream<TcpStream>),
    #[cfg(feature = "http3")]
    /// A Udp connection
    Udp(ConnectedUdpSocket),
}

impl AsyncRead for MixStream {
    // poll_read separately.
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_read(cx, buf),
            MixStream::Https(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "http3")]
            MixStream::Udp(s) => Pin::new(s).poll_recv(cx, buf),
        }
    }
}

impl AsyncWrite for MixStream {
    // poll_write separately.
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_write(ctx, buf),
            MixStream::Https(s) => Pin::new(s).poll_write(ctx, buf),
            #[cfg(feature = "http3")]
            MixStream::Udp(s) => Pin::new(s).poll_send(ctx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_flush(ctx),
            MixStream::Https(s) => Pin::new(s).poll_flush(ctx),
            #[cfg(feature = "http3")]
            MixStream::Udp(_) => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            MixStream::Http(s) => Pin::new(s).poll_shutdown(ctx),
            MixStream::Https(s) => Pin::new(s).poll_shutdown(ctx),
            #[cfg(feature = "http3")]
            MixStream::Udp(_) => Poll::Ready(Ok(())),
        }
    }
}
