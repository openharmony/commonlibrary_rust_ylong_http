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

#[cfg(feature = "http3")]
use crate::async_impl::quic::QuicConn;
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};
use crate::util::{ConnData, ConnInfo};

/// A connection wrapper containing io and io information.
pub struct HttpStream<T> {
    conn_data: ConnData,
    stream: T,
    #[cfg(feature = "http3")]
    quic_conn: Option<QuicConn>,
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
        self.conn_data.is_proxy()
    }

    fn conn_data(&self) -> ConnData {
        self.conn_data.clone()
    }

    #[cfg(feature = "http3")]
    fn quic_conn(&mut self) -> Option<QuicConn> {
        self.quic_conn.take()
    }
}

impl<T> HttpStream<T> {
    /// HttpStream constructor.
    pub(crate) fn new(io: T, conn_data: ConnData) -> HttpStream<T> {
        HttpStream {
            conn_data,
            stream: io,
            #[cfg(feature = "http3")]
            quic_conn: None,
        }
    }

    #[cfg(feature = "http3")]
    pub(crate) fn set_quic_conn(&mut self, conn: QuicConn) {
        self.quic_conn = Some(conn);
    }

    #[cfg(feature = "http3")]
    pub(crate) fn set_conn_data(&mut self, conn_data: ConnData) {
        self.conn_data = conn_data
    }
}
