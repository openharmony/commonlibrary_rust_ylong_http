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
use core::{future, ptr, slice};
use std::io::{self, Read, Write};

use crate::async_impl::ssl_stream::{check_io_to_poll, Wrapper};
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};
use crate::util::c_openssl::error::ErrorStack;
use crate::util::c_openssl::ssl::{self, ShutdownResult, Ssl, SslErrorCode};

/// An asynchronous version of [`openssl::ssl::SslStream`].
#[derive(Debug)]
pub struct AsyncSslStream<S>(ssl::SslStream<Wrapper<S>>);

impl<S> AsyncSslStream<S> {
    fn with_context<F, R>(self: Pin<&mut Self>, ctx: &mut Context<'_>, f: F) -> R
    where
        F: FnOnce(&mut ssl::SslStream<Wrapper<S>>) -> R,
    {
        // SAFETY: must guarantee that you will never move the data out of the
        // mutable reference you receive.
        let this = unsafe { self.get_unchecked_mut() };

        // sets context, SslStream to R, reset 0.
        this.0.get_mut().context = ctx as *mut _ as *mut ();
        let r = f(&mut this.0);
        this.0.get_mut().context = ptr::null_mut();
        r
    }

    /// Returns a pinned mutable reference to the underlying stream.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        // SAFETY:
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().0.get_mut().stream) }
    }

    pub(crate) fn negotiated_alpn_protocol(&self) -> Option<&[u8]> {
        self.0.ssl().negotiated_alpn_protocol()
    }
}

impl<S> AsyncSslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// Like [`SslStream::new`](ssl::SslStream::new).
    pub(crate) fn new(
        ssl: Ssl,
        stream: S,
        pinned_pubkey: Option<String>,
    ) -> Result<Self, ErrorStack> {
        // This corresponds to `SSL_set_bio`.
        ssl::SslStream::new_base(
            ssl,
            Wrapper {
                stream,
                context: ptr::null_mut(),
            },
            pinned_pubkey,
        )
        .map(AsyncSslStream)
    }

    /// Like [`SslStream::connect`](ssl::SslStream::connect).
    fn poll_connect(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), ssl::SslError>> {
        self.with_context(cx, |s| check_result_to_poll(s.connect()))
    }

    /// A convenience method wrapping [`poll_connect`](Self::poll_connect).
    pub(crate) async fn connect(mut self: Pin<&mut Self>) -> Result<(), ssl::SslError> {
        future::poll_fn(|cx| self.as_mut().poll_connect(cx)).await
    }
}

impl<S> AsyncRead for AsyncSslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    // wrap read.
    fn poll_read(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // set async func
        self.with_context(ctx, |s| {
            let slice = unsafe {
                let buf = buf.unfilled_mut();
                slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len())
            };
            match check_io_to_poll(s.read(slice))? {
                Poll::Ready(len) => {
                    #[cfg(feature = "tokio_base")]
                    unsafe {
                        buf.assume_init(len);
                    }
                    #[cfg(feature = "ylong_base")]
                    buf.assume_init(len);

                    buf.advance(len);
                    Poll::Ready(Ok(()))
                }
                Poll::Pending => Poll::Pending,
            }
        })
    }
}

impl<S> AsyncWrite for AsyncSslStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn poll_write(self: Pin<&mut Self>, ctx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.with_context(ctx, |s| check_io_to_poll(s.write(buf)))
    }

    fn poll_flush(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<io::Result<()>> {
        self.with_context(ctx, |s| check_io_to_poll(s.flush()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<io::Result<()>> {
        // Shuts down the session.
        match self.as_mut().with_context(ctx, |s| s.shutdown()) {
            // Sends a close notify message to the peer, after which `ShutdownResult::Sent` is
            // returned. Awaits the receipt of a close notify message from the peer,
            // after which `ShutdownResult::Received` is returned.
            Ok(ShutdownResult::Sent) | Ok(ShutdownResult::Received) => {}
            // The SSL session has been closed.
            Err(ref e) if e.code() == SslErrorCode::ZERO_RETURN => {}
            // When the underlying BIO could not satisfy the needs of SSL_shutdown() to continue the
            // handshake
            Err(ref e)
                if e.code() == SslErrorCode::WANT_READ || e.code() == SslErrorCode::WANT_WRITE =>
            {
                return Poll::Pending;
            }
            // Really error.
            Err(e) => {
                return Poll::Ready(Err(e
                    .into_io_error()
                    .unwrap_or_else(|e| io::Error::new(io::ErrorKind::Other, e))));
            }
        }
        // Returns success when the I/O connection has completely shut down.
        self.get_pin_mut().poll_shutdown(ctx)
    }
}

/// Checks `ssl::Error`.
fn check_result_to_poll<T>(r: Result<T, ssl::SslError>) -> Poll<Result<T, ssl::SslError>> {
    match r {
        Ok(t) => Poll::Ready(Ok(t)),
        Err(e) => match e.code() {
            SslErrorCode::WANT_READ | SslErrorCode::WANT_WRITE => Poll::Pending,
            _ => Poll::Ready(Err(e)),
        },
    }
}
