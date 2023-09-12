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

//! HTTP [`Content`] implementation.
//!
//! # Introduction
//!
//! HTTP messages often transfer a complete or partial representation as the
//! message content: a stream of octets sent after the header section, as
//! delineated by the message framing.
//!
//! This abstract definition of content reflects the data after it has been
//! extracted from the message framing. For example, an `HTTP/1.1` message body
//! might consist of a stream of data encoded with the chunked transfer coding —
//! a sequence of data chunks, one zero-length chunk, and a trailer section —
//! whereas the content of that same message includes only the data stream after
//! the transfer coding has been decoded; it does not include the chunk lengths,
//! chunked framing syntax, nor the trailer fields.
//!
//! [`Content`]: https://httpwg.org/specs/rfc9110.html#content
//!
//! # Various Body Types
//!
//! This module provides following body types:
//!
//! - [`EmptyBody`]: `EmptyBody` represents an empty body.
//! - [`TextBody`]: `TextBody` represents a plain-text body.
//!
//! [`EmptyBody`]: EmptyBody
//! [`TextBody`]: TextBody

// TODO: Support `Trailers`.

mod chunk;
mod empty;
mod mime;
mod text;

pub use chunk::{Chunk, ChunkBody, ChunkBodyDecoder, ChunkExt, ChunkState, Chunks};
pub use empty::EmptyBody;
pub use mime::{
    MimeMulti, MimeMultiBuilder, MimeMultiDecoder, MimeMultiEncoder, MimePart, MimePartBuilder,
    MimePartEncoder, MimeType, MultiPart, MultiPartBase, Part, TokenStatus, XPart,
};
pub use text::{Text, TextBody, TextBodyDecoder};

/// Synchronous `Body` trait definition.
pub mod sync_impl {
    use std::error::Error;
    use std::io::Read;

    use crate::headers::Headers;

    /// The `sync_impl::Body` trait allows for reading body data synchronously.
    ///
    /// # Examples
    ///
    /// [`TextBody`] implements `sync_impl::Body`:
    ///
    /// ```
    /// use ylong_http::body::sync_impl::Body;
    /// use ylong_http::body::TextBody;
    ///
    /// // `TextBody` has 5-bytes length content.
    /// let mut body = TextBody::from_bytes(b"Hello");
    ///
    /// // We can use any non-zero length `buf` to read it.
    /// let mut buf = [0u8; 4];
    ///
    /// // Read 4 bytes. `buf` is filled.
    /// // The remaining 1 bytes of `TextBody` have not been read.
    /// let read = body.data(&mut buf).unwrap();
    /// assert_eq!(read, 4);
    /// assert_eq!(&buf[..read], b"Hell");
    ///
    /// // Read next 1 bytes. Part of `buf` is filled.
    /// // All bytes of `TextBody` have been read.
    /// let read = body.data(&mut buf).unwrap();
    /// assert_eq!(read, 1);
    /// assert_eq!(&buf[..read], b"o");
    ///
    /// // The `TextBody` has already been read, and no more content can be read.
    /// let read = body.data(&mut buf).unwrap();
    /// assert_eq!(read, 0);
    /// ```
    ///
    /// [`TextBody`]: super::text::TextBody
    pub trait Body {
        /// Errors that may occur when reading body data.
        type Error: Into<Box<dyn Error + Send + Sync>>;

        /// Synchronously reads part of the body data, returning how many bytes
        /// were read. Body data will be written into buf as much as possible.
        ///
        /// # Return Value
        ///
        /// - `Ok(0)`:
        /// If the length of the `buf` is not 0, it means that this
        /// body has been completely read.
        ///
        /// - `Ok(size)` && `size != 0`:
        /// A part of this body has been read, but the body may not be fully
        /// read. You can call this method again to obtain next part of data.
        ///
        /// - `Err(e)`:
        /// An error occurred while reading body data.
        ///
        /// # Note
        ///
        /// It is better for you **not** to use a `buf` with a length of 0,
        /// otherwise it may lead to misunderstanding of the return value.
        ///
        /// # Examples
        ///
        /// [`TextBody`] implements `sync_impl::Body`:
        ///
        /// ```
        /// use ylong_http::body::sync_impl::Body;
        /// use ylong_http::body::TextBody;
        ///
        /// // `TextBody` has 5-bytes length content.
        /// let mut body = TextBody::from_bytes(b"Hello");
        ///
        /// // We can use any non-zero length `buf` to read it.
        /// let mut buf = [0u8; 4];
        ///
        /// // Read 4 bytes. `buf` is filled.
        /// // The remaining 1 bytes of `TextBody` have not been read.
        /// let read = body.data(&mut buf).unwrap();
        /// assert_eq!(read, 4);
        /// assert_eq!(&buf[..read], b"Hell");
        ///
        /// // Read next 1 bytes. Part of `buf` is filled.
        /// // All bytes of `TextBody` have been read.
        /// let read = body.data(&mut buf).unwrap();
        /// assert_eq!(read, 1);
        /// assert_eq!(&buf[..read], b"o");
        ///
        /// // The `TextBody` has already been read, and no more content can be read.
        /// let read = body.data(&mut buf).unwrap();
        /// assert_eq!(read, 0);
        /// ```
        ///
        /// [`TextBody`]: super::text::TextBody
        fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;

        /// Gets trailer headers.
        fn trailer(&mut self) -> Result<Option<Headers>, Self::Error> {
            Ok(None)
        }
    }

    impl<T: Read> Body for T {
        type Error = std::io::Error;

        fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            self.read(buf)
        }
    }
}

/// Asynchronous `Body` trait definition.
pub mod async_impl {
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};
    use std::error::Error;

    use crate::headers::Headers;
    use crate::{AsyncRead, ReadBuf};

    /// The `async_impl::Body` trait allows for reading body data
    /// asynchronously.
    ///
    /// # Examples
    ///
    /// [`TextBody`] implements `async_impl::Body`:
    ///
    /// ```
    /// use ylong_http::body::async_impl::Body;
    /// use ylong_http::body::TextBody;
    ///
    /// # async fn read_body_data() {
    /// // `TextBody` has 5-bytes length content.
    /// let mut body = TextBody::from_bytes(b"Hello");
    ///
    /// // We can use any non-zero length `buf` to read it.
    /// let mut buf = [0u8; 4];
    ///
    /// // Read 4 bytes. `buf` is filled.
    /// // The remaining 1 bytes of `TextBody` have not been read.
    /// let read = body.data(&mut buf).await.unwrap();
    /// assert_eq!(read, 4);
    /// assert_eq!(&buf[..read], b"Hell");
    ///
    /// // Read next 1 bytes. Part of `buf` is filled.
    /// // All bytes of `TextBody` have been read.
    /// let read = body.data(&mut buf).await.unwrap();
    /// assert_eq!(read, 1);
    /// assert_eq!(&buf[..read], b"o");
    ///
    /// // The `TextBody` has already been read, and no more content can be read.
    /// let read = body.data(&mut buf).await.unwrap();
    /// assert_eq!(read, 0);
    /// # }
    /// ```
    ///
    /// [`TextBody`]: super::text::TextBody
    pub trait Body: Unpin + Sized {
        /// Errors that may occur when reading body data.
        type Error: Into<Box<dyn Error + Send + Sync>>;

        /// Reads part of the body data, returning how many bytes were read.
        ///
        /// Body data will be written into buf as much as possible.
        fn poll_data(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize, Self::Error>>;

        /// Returns a future that reads part of the body data, returning how
        /// many bytes were read. Body data will be written into buf as much
        /// as possible.
        ///
        /// # Return Value
        ///
        /// - `Ok(0)`:
        /// If the length of the `buf` is not 0, it means that this body has
        /// been completely read.
        ///
        /// - `Ok(size)` && `size != 0`:
        /// A part of this body has been read, but the body may not be
        /// completely read. You can call this method again to obtain next part
        /// of data.
        ///
        /// - `Err(e)`:
        /// An error occurred while reading body data.
        ///
        /// # Note
        ///
        /// It is better for you **not** to use a `buf` with a length of 0,
        /// otherwise it may lead to misunderstanding of the return value.
        ///
        /// # Examples
        ///
        /// [`TextBody`] implements `async_impl::Body`:
        ///
        /// ```
        /// use ylong_http::body::async_impl::Body;
        /// use ylong_http::body::TextBody;
        ///
        /// # async fn read_body_data() {
        /// // `TextBody` has 5-bytes length content.
        /// let mut body = TextBody::from_bytes(b"Hello");
        ///
        /// // We can use any non-zero length `buf` to read it.
        /// let mut buf = [0u8; 4];
        ///
        /// // Read 4 bytes. `buf` is filled.
        /// // The remaining 1 bytes of `TextBody` have not been read.
        /// let read = body.data(&mut buf).await.unwrap();
        /// assert_eq!(read, 4);
        /// assert_eq!(&buf[..read], b"Hell");
        ///
        /// // Read next 1 bytes. Part of `buf` is filled.
        /// // All bytes of `TextBody` have been read.
        /// let read = body.data(&mut buf).await.unwrap();
        /// assert_eq!(read, 1);
        /// assert_eq!(&buf[..read], b"o");
        ///
        /// // The `TextBody` has already been read, and no more content can be read.
        /// let read = body.data(&mut buf).await.unwrap();
        /// assert_eq!(read, 0);
        /// # }
        /// ```
        ///
        /// [`TextBody`]: super::text::TextBody
        fn data<'a, 'b>(&'a mut self, buf: &'b mut [u8]) -> DataFuture<'a, 'b, Self>
        where
            Self: 'a,
            Self: 'b,
        {
            DataFuture { body: self, buf }
        }

        /// Gets trailer headers.
        fn poll_trailer(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<Option<Headers>, Self::Error>> {
            Poll::Ready(Ok(None))
        }

        /// Returns a future that reads part of the trailer data, returning the
        /// headers which were read. Trialer data will be written into
        /// buf as headers.
        ///
        /// # Return Value
        ///
        /// - `Ok(Some(headers))`:
        /// If the trailer has been completely read, headers will be returned.
        ///
        /// - `Ok(None)`:
        /// If return none, means trailer is empty.
        ///
        /// - `Err(e)`:
        /// An error occurred while reading trailer data.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http::body::async_impl::Body;
        /// use ylong_http::body::ChunkBody;
        /// # async fn read_trailer_data() {
        /// let box_stream = Box::new("".as_bytes());
        /// // Chunk body contain trailer data
        /// let chunk_body_bytes = "\
        ///             5\r\n\
        ///             hello\r\n\
        ///             C ; type = text ;end = !\r\n\
        ///             hello world!\r\n\
        ///             000; message = last\r\n\
        ///             accept:text/html\r\n\r\n\
        ///             ";
        ///
        /// // Gets `ChunkBody`
        /// let mut chunk = ChunkBody::from_bytes(chunk_body_bytes.as_bytes());
        /// // read chunk body and return headers
        /// let res = chunk.trailer().await.unwrap().unwrap();
        /// assert_eq!(
        ///     res.get("accept").unwrap().to_string().unwrap(),
        ///     "text/html".to_string()
        /// );
        /// # }
        /// ```
        fn trailer<'a>(&'a mut self) -> TrailerFuture<'a, Self>
        where
            Self: 'a,
        {
            TrailerFuture { body: self }
        }
    }

    /// A future that reads data from trailer, returning whole headers
    /// were read.
    ///
    /// This future is the return value of `async_impl::Body::trailer`.
    pub struct TrailerFuture<'a, T>
    where
        T: Body + 'a,
    {
        body: &'a mut T,
    }

    impl<'a, T> Future for TrailerFuture<'a, T>
    where
        T: Body + 'a,
    {
        type Output = Result<Option<Headers>, T::Error>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let fut = self.get_mut();
            Pin::new(&mut *fut.body).poll_trailer(cx)
        }
    }

    /// A future that reads part of the body data, returning how many bytes
    /// were read.
    ///
    /// This future is the return value of `async_impl::Body::data`.
    ///
    /// [`async_impl::Body::data`]: Body::data
    pub struct DataFuture<'a, 'b, T>
    where
        T: Body + 'a + 'b,
    {
        body: &'a mut T,
        buf: &'b mut [u8],
    }

    impl<'a, 'b, T> Future for DataFuture<'a, 'b, T>
    where
        T: Body + 'a + 'b,
    {
        type Output = Result<usize, T::Error>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let fut = self.get_mut();
            Pin::new(&mut *fut.body).poll_data(cx, fut.buf)
        }
    }
}

// Type definitions of the origin of the body data.
pub(crate) mod origin {
    use core::ops::{Deref, DerefMut};
    use std::io::Read;

    use crate::AsyncRead;

    /// A type that represents the body data is from memory.
    pub struct FromBytes<'a> {
        pub(crate) bytes: &'a [u8],
    }

    impl<'a> FromBytes<'a> {
        pub(crate) fn new(bytes: &'a [u8]) -> Self {
            Self { bytes }
        }
    }

    impl<'a> Deref for FromBytes<'a> {
        type Target = &'a [u8];

        fn deref(&self) -> &Self::Target {
            &self.bytes
        }
    }

    impl<'a> DerefMut for FromBytes<'a> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.bytes
        }
    }

    /// A type that represents the body data is from a synchronous reader.
    pub struct FromReader<T: Read> {
        pub(crate) reader: T,
    }

    impl<T: Read> FromReader<T> {
        pub(crate) fn new(reader: T) -> Self {
            Self { reader }
        }
    }

    impl<T: Read> Deref for FromReader<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.reader
        }
    }

    impl<T: Read> DerefMut for FromReader<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.reader
        }
    }

    /// A type that represent the body data is from an asynchronous reader.
    pub struct FromAsyncReader<T: AsyncRead + Unpin + Send + Sync> {
        pub(crate) reader: T,
    }

    impl<T: AsyncRead + Unpin + Send + Sync> FromAsyncReader<T> {
        pub(crate) fn new(reader: T) -> Self {
            Self { reader }
        }
    }

    impl<T: AsyncRead + Unpin + Send + Sync> Deref for FromAsyncReader<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.reader
        }
    }

    impl<T: AsyncRead + Unpin + Send + Sync> DerefMut for FromAsyncReader<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.reader
        }
    }

    /// A type that represents the body data is from an asynchronous body.
    pub struct FromAsyncBody<T: super::async_impl::Body> {
        pub(crate) body: T,
    }

    impl<T: super::async_impl::Body> FromAsyncBody<T> {
        pub(crate) fn new(body: T) -> Self {
            Self { body }
        }
    }

    impl<T: super::async_impl::Body> Deref for FromAsyncBody<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.body
        }
    }

    impl<T: super::async_impl::Body> DerefMut for FromAsyncBody<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.body
        }
    }
}

#[cfg(test)]
mod ut_mod {
    use crate::body::EmptyBody;

    /// UT test cases for `sync_impl::Body::data` of `&mut sync_impl::Body`.
    ///
    /// # Brief
    /// 1. Creates a `sync_impl::Body` object.
    /// 2. Gets its mutable reference.
    /// 3. Calls its `sync_impl::Body::data` method and then checks the results.
    #[test]
    fn ut_syn_body_mut_syn_body_data() {
        use crate::body::sync_impl::Body;

        let mut body = EmptyBody::new();
        let body_mut = &mut body;
        let mut buf = [0u8; 1];
        assert_eq!(body_mut.data(&mut buf), Ok(0));
    }

    /// UT test cases for `async_impl::Body::data` of `&mut async_impl::Body`.
    ///
    /// # Brief
    /// 1. Creates a `async_impl::Body` object.
    /// 2. Gets its mutable reference.
    /// 3. Calls its `async_impl::Body::data` method and then checks the
    ///    results.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_asyn_body_mut_asyn_body_data() {
        let handle = ylong_runtime::spawn(async move {
            asyn_body_mut_asyn_body_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn asyn_body_mut_asyn_body_data() {
        use crate::body::async_impl::Body;

        let mut body = EmptyBody::new();
        let body_mut = &mut body;
        let mut buf = [0u8; 1];
        assert_eq!(body_mut.data(&mut buf).await, Ok(0));
    }
}
