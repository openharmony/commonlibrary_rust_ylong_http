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

//! `ConnDetail` trait and `HttpStream` implementation.

use std::pin::Pin;
use std::task::{Context, Poll};

use crate::async_impl::interceptor::ConnDetail;
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};

/// `ConnDetail` trait, which is used to obtain information about the current
/// connection.
pub trait ConnInfo {
    /// Whether the current connection is a proxy.
    fn is_proxy(&self) -> bool;

    fn conn_detail(&self) -> ConnDetail;
}

/// A connection wrapper containing io and io information.
pub struct HttpStream<T> {
    detail: ConnDetail,
    stream: T,
}

impl<T> AsyncRead for HttpStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // poll_read separately.
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl<T> AsyncWrite for HttpStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // poll_write separately.
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl<T> ConnInfo for HttpStream<T> {
    fn is_proxy(&self) -> bool {
        self.detail.proxy
    }

    fn conn_detail(&self) -> ConnDetail {
        self.detail.clone()
    }
}

impl<T> HttpStream<T> {
    /// HttpStream constructor.
    pub fn new(io: T, detail: ConnDetail) -> HttpStream<T> {
        HttpStream { detail, stream: io }
    }
}
