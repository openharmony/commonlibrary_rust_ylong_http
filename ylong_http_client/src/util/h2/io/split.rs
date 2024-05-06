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

//! implement `split` fn for io, split it into `Reader` half and `Writer` half.

use std::io;
use std::io::IoSlice;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use ylong_runtime::io::{AsyncRead, AsyncWrite, ReadBuf};
use ylong_runtime::sync::{Mutex, MutexGuard};

macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            std::task::Poll::Ready(t) => t,
            std::task::Poll::Pending => return std::task::Poll::Pending,
        }
    };
}

pub(crate) struct Reader<T> {
    inner: Arc<InnerLock<T>>,
}

pub(crate) struct Writer<T> {
    inner: Arc<InnerLock<T>>,
}

struct InnerLock<T> {
    stream: Mutex<T>,
    is_write_vectored: bool,
}

struct StreamGuard<'a, T> {
    inner: MutexGuard<'a, T>,
}

pub(crate) fn split<T>(stream: T) -> (Reader<T>, Writer<T>)
where
    T: AsyncRead + AsyncWrite,
{
    let is_write_vectored = stream.is_write_vectored();
    let inner = Arc::new(InnerLock {
        stream: Mutex::new(stream),
        is_write_vectored,
    });

    let rd = Reader {
        inner: inner.clone(),
    };

    let wr = Writer { inner };

    (rd, wr)
}

impl<T: AsyncRead> AsyncRead for Reader<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut guard = ready!(self.inner.get_lock(cx));
        guard.stream().poll_read(cx, buf)
    }
}

impl<T: AsyncWrite> AsyncWrite for Writer<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut inner = ready!(self.inner.get_lock(cx));
        inner.stream().poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let mut inner = ready!(self.inner.get_lock(cx));
        inner.stream().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut inner = ready!(self.inner.get_lock(cx));
        inner.stream().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut inner = ready!(self.inner.get_lock(cx));
        inner.stream().poll_shutdown(cx)
    }
}

impl<'a, T> StreamGuard<'a, T> {
    fn stream(&mut self) -> Pin<&mut T> {
        // safety: the stream is pinned in `Arc` and the `Guard` ensures mutual
        // exclusion.
        unsafe { Pin::new_unchecked(&mut *self.inner) }
    }
}

impl<T> InnerLock<T> {
    fn get_lock(&self, cx: &mut Context<'_>) -> Poll<StreamGuard<T>> {
        match self.stream.try_lock() {
            Ok(guard) => Poll::Ready(StreamGuard { inner: guard }),
            Err(_) => {
                std::thread::yield_now();
                cx.waker().wake_by_ref();

                Poll::Pending
            }
        }
    }
}
