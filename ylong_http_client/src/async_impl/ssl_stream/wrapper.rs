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

use core::fmt::Debug;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io::{self, Read, Write};

use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Debug)]
pub(crate) struct Wrapper<S> {
    pub(crate) stream: S,
    // Context of stream.
    pub(crate) context: *mut (),
}

impl<S> Wrapper<S> {
    /// Gets inner `Stream` and `Context` of `Stream`.
    ///
    /// # SAFETY
    /// Must be called with `context` set to a valid pointer to a live `Context`
    /// object, and the wrapper must be pinned in memory.
    unsafe fn inner(&mut self) -> (Pin<&mut S>, &mut Context<'_>) {
        debug_assert!(!self.context.is_null());
        let stream = Pin::new_unchecked(&mut self.stream);
        let context = &mut *(self.context as *mut _);
        (stream, context)
    }
}

impl<S> Read for Wrapper<S>
where
    S: AsyncRead,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let (stream, cx) = unsafe { self.inner() };
        let mut buf = ReadBuf::new(buf);
        match stream.poll_read(cx, &mut buf)? {
            Poll::Ready(()) => Ok(buf.filled().len()),
            Poll::Pending => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        }
    }
}

impl<S> Write for Wrapper<S>
where
    S: AsyncWrite,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (stream, cx) = unsafe { self.inner() };
        match stream.poll_write(cx, buf) {
            Poll::Ready(r) => r,
            Poll::Pending => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let (stream, cx) = unsafe { self.inner() };
        match stream.poll_flush(cx) {
            Poll::Ready(r) => r,
            Poll::Pending => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        }
    }
}

// *mut () is not impl Send or Sync.
unsafe impl<S: Send> Send for Wrapper<S> {}
unsafe impl<S: Sync> Sync for Wrapper<S> {}

/// Checks `io::Result`.
pub(crate) fn check_io_to_poll<T>(r: io::Result<T>) -> Poll<io::Result<T>> {
    match r {
        Ok(t) => Poll::Ready(Ok(t)),
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
        Err(e) => Poll::Ready(Err(e)),
    }
}
