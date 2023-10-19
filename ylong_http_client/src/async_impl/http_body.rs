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
use std::future::Future;
use std::io::{Cursor, Read};
use std::sync::Arc;

use ylong_http::body::async_impl::Body;
use ylong_http::body::TextBodyDecoder;
#[cfg(feature = "http1_1")]
use ylong_http::body::{ChunkBodyDecoder, ChunkState};
use ylong_http::headers::Headers;

use super::conn::StreamData;
use crate::async_impl::interceptor::Interceptors;
use crate::error::{ErrorKind, HttpClientError};
use crate::runtime::{AsyncRead, ReadBuf, Sleep};
use crate::util::normalizer::BodyLength;

const TRAILER_SIZE: usize = 1024;

/// `HttpBody` is the body part of the `Response` returned by `Client::request`.
/// `HttpBody` implements `Body` trait, so users can call related methods to get
/// body data.
///
/// # Examples
///
/// ```no_run
/// use ylong_http_client::async_impl::{Body, Client, HttpBody, Request};
/// use ylong_http_client::HttpClientError;
///
/// async fn read_body() -> Result<(), HttpClientError> {
///     let client = Client::new();
///
///     // `HttpBody` is the body part of `response`.
///     let mut response = client
///         .request(Request::builder().body(Body::empty())?)
///         .await?;
///
///     // Users can use `Body::data` to get body data.
///     let mut buf = [0u8; 1024];
///     loop {
///         let size = response.data(&mut buf).await.unwrap();
///         if size == 0 {
///             break;
///         }
///         let _data = &buf[..size];
///         // Deals with the data.
///     }
///     Ok(())
/// }
/// ```
pub struct HttpBody {
    kind: Kind,
    sleep: Option<Pin<Box<Sleep>>>,
}

type BoxStreamData = Box<dyn StreamData + Sync + Send + Unpin>;

impl HttpBody {
    pub(crate) fn new(
        interceptors: Arc<Interceptors>,
        body_length: BodyLength,
        io: BoxStreamData,
        pre: &[u8],
    ) -> Result<Self, HttpClientError> {
        let kind = match body_length {
            BodyLength::Empty => {
                if !pre.is_empty() {
                    // TODO: Consider the case where BodyLength is empty but pre is not empty.
                    io.shutdown();
                    return err_from_msg!(Request, "Body length is 0 but read extra data");
                }
                Kind::Empty
            }
            BodyLength::Length(len) => Kind::Text(Text::new(len, pre, io, interceptors)),
            BodyLength::UntilClose => Kind::UntilClose(UntilClose::new(pre, io, interceptors)),

            #[cfg(feature = "http1_1")]
            BodyLength::Chunk => Kind::Chunk(Chunk::new(pre, io, interceptors)),
        };
        Ok(Self { kind, sleep: None })
    }

    pub(crate) fn set_sleep(&mut self, sleep: Option<Pin<Box<Sleep>>>) {
        self.sleep = sleep;
    }
}

impl Body for HttpBody {
    type Error = HttpClientError;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        if let Some(delay) = self.sleep.as_mut() {
            if let Poll::Ready(()) = Pin::new(delay).poll(cx) {
                return Poll::Ready(err_from_io!(Timeout, std::io::ErrorKind::TimedOut.into()));
            }
        }

        match self.kind {
            Kind::Empty => Poll::Ready(Ok(0)),
            Kind::Text(ref mut text) => text.data(cx, buf),
            Kind::UntilClose(ref mut until_close) => until_close.data(cx, buf),
            #[cfg(feature = "http1_1")]
            Kind::Chunk(ref mut chunk) => chunk.data(cx, buf),
        }
    }

    fn poll_trailer(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Headers>, Self::Error>> {
        // Get trailer data from io
        if let Some(delay) = self.sleep.as_mut() {
            if let Poll::Ready(()) = Pin::new(delay).poll(cx) {
                return Poll::Ready(err_from_msg!(Timeout, "Request timeout"));
            }
        }

        let mut read_buf = [0_u8; TRAILER_SIZE];

        match self.kind {
            #[cfg(feature = "http1_1")]
            Kind::Chunk(ref mut chunk) => {
                match chunk.data(cx, &mut read_buf) {
                    Poll::Ready(Ok(_)) => {}
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(Err(e));
                    }
                }
                Poll::Ready(Ok(chunk.decoder.get_trailer().map_err(|e| {
                    HttpClientError::from_error(ErrorKind::BodyDecode, e)
                })?))
            }
            _ => Poll::Ready(Ok(None)),
        }
    }
}

impl Drop for HttpBody {
    fn drop(&mut self) {
        let io = match self.kind {
            Kind::Text(ref mut text) => text.io.as_mut(),
            #[cfg(feature = "http1_1")]
            Kind::Chunk(ref mut chunk) => chunk.io.as_mut(),
            Kind::UntilClose(ref mut until_close) => until_close.io.as_mut(),
            _ => None,
        };
        // If response body is not totally read, shutdown io.
        if let Some(io) = io {
            io.shutdown()
        }
    }
}

// TODO: `TextBodyDecoder` implementation and `ChunkBodyDecoder` implementation.
enum Kind {
    Empty,
    Text(Text),
    #[cfg(feature = "http1_1")]
    Chunk(Chunk),
    UntilClose(UntilClose),
}

struct UntilClose {
    interceptors: Arc<Interceptors>,
    pre: Option<Cursor<Vec<u8>>>,
    io: Option<BoxStreamData>,
}

impl UntilClose {
    pub(crate) fn new(pre: &[u8], io: BoxStreamData, interceptors: Arc<Interceptors>) -> Self {
        Self {
            interceptors,
            pre: (!pre.is_empty()).then_some(Cursor::new(pre.to_vec())),
            io: Some(io),
        }
    }

    fn data(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, HttpClientError>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        let mut read = 0;

        if let Some(pre) = self.pre.as_mut() {
            // Here cursor read never failed.
            let this_read = Read::read(pre, buf).unwrap();
            if this_read == 0 {
                self.pre = None;
            } else {
                read += this_read;
            }
        }

        if !buf[read..].is_empty() {
            if let Some(mut io) = self.io.take() {
                let mut read_buf = ReadBuf::new(&mut buf[read..]);
                return match Pin::new(&mut io).poll_read(cx, &mut read_buf) {
                    // Disconnected.
                    Poll::Ready(Ok(())) => {
                        let filled = read_buf.filled().len();
                        if filled == 0 {
                            io.shutdown();
                        } else {
                            self.interceptors
                                .intercept_output(&buf[read..(read + filled)])?;
                            self.io = Some(io);
                        }
                        read += filled;
                        Poll::Ready(Ok(read))
                    }
                    Poll::Pending => {
                        self.io = Some(io);
                        if read != 0 {
                            return Poll::Ready(Ok(read));
                        }
                        Poll::Pending
                    }
                    Poll::Ready(Err(e)) => {
                        // If IO error occurs, shutdowns `io` before return.
                        io.shutdown();
                        return Poll::Ready(err_from_io!(BodyTransfer, e));
                    }
                };
            }
        }
        Poll::Ready(Ok(read))
    }
}

struct Text {
    interceptors: Arc<Interceptors>,
    decoder: TextBodyDecoder,
    pre: Option<Cursor<Vec<u8>>>,
    io: Option<BoxStreamData>,
}

impl Text {
    pub(crate) fn new(
        len: usize,
        pre: &[u8],
        io: BoxStreamData,
        interceptors: Arc<Interceptors>,
    ) -> Self {
        Self {
            interceptors,
            decoder: TextBodyDecoder::new(len),
            pre: (!pre.is_empty()).then_some(Cursor::new(pre.to_vec())),
            io: Some(io),
        }
    }
}

impl Text {
    fn data(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, HttpClientError>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        let mut read = 0;

        if let Some(pre) = self.pre.as_mut() {
            // Here cursor read never failed.
            let this_read = Read::read(pre, buf).unwrap();
            if this_read == 0 {
                self.pre = None;
            } else {
                read += this_read;
                let (text, rem) = self.decoder.decode(&buf[..read]);

                // Contains redundant `rem`, return error.
                match (text.is_complete(), rem.is_empty()) {
                    (true, false) => {
                        if let Some(io) = self.io.take() {
                            io.shutdown();
                        };
                        return Poll::Ready(err_from_msg!(BodyDecode, "Not eof"));
                    }
                    (true, true) => {
                        self.io = None;
                        return Poll::Ready(Ok(read));
                    }
                    // TextBodyDecoder decodes as much as possible here.
                    _ => {}
                }
            }
        }

        if !buf[read..].is_empty() {
            if let Some(mut io) = self.io.take() {
                let mut read_buf = ReadBuf::new(&mut buf[read..]);
                match Pin::new(&mut io).poll_read(cx, &mut read_buf) {
                    // Disconnected.
                    Poll::Ready(Ok(())) => {
                        let filled = read_buf.filled().len();
                        if filled == 0 {
                            io.shutdown();
                            return Poll::Ready(err_from_msg!(
                                BodyDecode,
                                "Response body incomplete"
                            ));
                        }
                        let (text, rem) = self.decoder.decode(read_buf.filled());
                        self.interceptors.intercept_output(read_buf.filled())?;
                        read += filled;
                        // Contains redundant `rem`, return error.
                        match (text.is_complete(), rem.is_empty()) {
                            (true, false) => {
                                io.shutdown();
                                return Poll::Ready(err_from_msg!(BodyDecode, "Not eof"));
                            }
                            (true, true) => return Poll::Ready(Ok(read)),
                            _ => {}
                        }
                        self.io = Some(io);
                    }
                    Poll::Pending => {
                        self.io = Some(io);
                        if read != 0 {
                            return Poll::Ready(Ok(read));
                        }
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => {
                        // If IO error occurs, shutdowns `io` before return.
                        io.shutdown();
                        return Poll::Ready(err_from_io!(BodyDecode, e));
                    }
                }
            }
        }
        Poll::Ready(Ok(read))
    }
}

#[cfg(feature = "http1_1")]
struct Chunk {
    interceptors: Arc<Interceptors>,
    decoder: ChunkBodyDecoder,
    pre: Option<Cursor<Vec<u8>>>,
    io: Option<BoxStreamData>,
}

#[cfg(feature = "http1_1")]
impl Chunk {
    pub(crate) fn new(pre: &[u8], io: BoxStreamData, interceptors: Arc<Interceptors>) -> Self {
        Self {
            interceptors,
            decoder: ChunkBodyDecoder::new().contains_trailer(true),
            pre: (!pre.is_empty()).then_some(Cursor::new(pre.to_vec())),
            io: Some(io),
        }
    }
}

#[cfg(feature = "http1_1")]
impl Chunk {
    fn data(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, HttpClientError>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        let mut read = 0;

        while let Some(pre) = self.pre.as_mut() {
            // Here cursor read never failed.
            let size = Read::read(pre, &mut buf[read..]).unwrap();
            if size == 0 {
                self.pre = None;
            }

            let (size, flag) = self.merge_chunks(&mut buf[read..read + size])?;
            read += size;

            if flag {
                // Return if we find a 0-sized chunk.
                self.io = None;
                return Poll::Ready(Ok(read));
            } else if read != 0 {
                // Return if we get some data.
                return Poll::Ready(Ok(read));
            }
        }

        // Here `read` must be 0.
        while let Some(mut io) = self.io.take() {
            let mut read_buf = ReadBuf::new(&mut buf[read..]);
            match Pin::new(&mut io).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) => {
                    let filled = read_buf.filled().len();
                    if filled == 0 {
                        io.shutdown();
                        return Poll::Ready(err_from_msg!(BodyDecode, "Response body incomplete"));
                    }
                    let (size, flag) = self.merge_chunks(read_buf.filled_mut())?;
                    self.interceptors.intercept_output(read_buf.filled_mut())?;
                    read += size;
                    if flag {
                        // Return if we find a 0-sized chunk.
                        // Return if we get some data.
                        return Poll::Ready(Ok(read));
                    }
                    self.io = Some(io);
                    if read != 0 {
                        return Poll::Ready(Ok(read));
                    }
                }
                Poll::Pending => {
                    self.io = Some(io);
                    return Poll::Pending;
                }
                Poll::Ready(Err(e)) => {
                    // If IO error occurs, shutdowns `io` before return.
                    io.shutdown();
                    return Poll::Ready(err_from_io!(BodyDecode, e));
                }
            }
        }

        Poll::Ready(Ok(read))
    }

    fn merge_chunks(&mut self, buf: &mut [u8]) -> Result<(usize, bool), HttpClientError> {
        // Here we need to merge the chunks into one data block and return.
        // The data arrangement in buf is as follows:
        //
        // data in buf:
        // +------+------+------+------+------+------+------+
        // | data | len  | data | len  |  ... | data |  len |
        // +------+------+------+------+------+------+------+
        //
        // We need to merge these data blocks into one block:
        //
        // after merge:
        // +---------------------------+
        // |            data           |
        // +---------------------------+

        let (chunks, junk) = self
            .decoder
            .decode(buf)
            .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;

        let mut finished = false;
        let mut ptrs = Vec::new();

        for chunk in chunks.into_iter() {
            if chunk.trailer().is_some() {
                if chunk.state() == &ChunkState::Finish {
                    finished = true;
                }
            } else {
                if chunk.size() == 0 && chunk.state() != &ChunkState::MetaSize {
                    finished = true;
                    break;
                }
                let data = chunk.data();
                ptrs.push((data.as_ptr(), data.len()))
            }
        }

        if finished && !junk.is_empty() {
            return err_from_msg!(BodyDecode, "Invalid chunk body");
        }

        let start = buf.as_ptr();

        let mut idx = 0;
        for (ptr, len) in ptrs.into_iter() {
            let st = ptr as usize - start as usize;
            let ed = st + len;
            buf.copy_within(st..ed, idx);
            idx += len;
        }
        Ok((idx, finished))
    }
}

#[cfg(feature = "ylong_base")]
#[cfg(test)]
mod ut_async_http_body {
    use std::sync::Arc;

    use ylong_http::body::async_impl;

    use crate::async_impl::interceptor::IdleInterceptor;
    use crate::async_impl::HttpBody;
    use crate::util::normalizer::BodyLength;
    use crate::ErrorKind;

    /// UT test cases for `HttpBody::trailer`.
    ///
    /// # Brief
    /// 1. Creates a `HttpBody` by calling `HttpBody::new`.
    /// 2. Calls `trailer` to get headers.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_asnyc_chunk_trailer_1() {
        let handle = ylong_runtime::spawn(async move {
            async_chunk_trailer_1().await;
            async_chunk_trailer_2().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn async_chunk_trailer_1() {
        let box_stream = Box::new("".as_bytes());
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            accept:text/html\r\n\r\n\
            ";
        let mut chunk = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Chunk,
            box_stream,
            chunk_body_bytes.as_bytes(),
        )
        .unwrap();
        let res = async_impl::Body::trailer(&mut chunk)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            res.get("accept").unwrap().to_string().unwrap(),
            "text/html".to_string()
        );
        let box_stream = Box::new("".as_bytes());
        let chunk_body_no_trailer_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            0\r\n\r\n\
            ";

        let mut chunk = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Chunk,
            box_stream,
            chunk_body_no_trailer_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 32];
        // Read body part
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf[..read], b"hello");
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 12);
        assert_eq!(&buf[..read], b"hello world!");
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 0);
        assert_eq!(&buf[..read], b"");
        // try read trailer part
        let res = async_impl::Body::trailer(&mut chunk).await.unwrap();
        assert!(res.is_none());
    }

    async fn async_chunk_trailer_2() {
        let box_stream = Box::new("".as_bytes());
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            Expires: Wed, 21 Oct 2015 07:27:00 GMT \r\n\r\n\
            ";
        let mut chunk = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Chunk,
            box_stream,
            chunk_body_bytes.as_bytes(),
        )
        .unwrap();
        let res = async_impl::Body::trailer(&mut chunk)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            res.get("expires").unwrap().to_string().unwrap(),
            "Wed, 21 Oct 2015 07:27:00 GMT".to_string()
        );
    }

    /// UT test cases for `Body::data`.
    ///
    /// # Brief
    /// 1. Creates a chunk `HttpBody`.
    /// 2. Calls `data` method get boxstream.
    /// 3. Checks if data size is correct.
    #[test]
    fn ut_asnyc_http_body_chunk2() {
        let handle = ylong_runtime::spawn(async move {
            http_body_chunk2().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn http_body_chunk2() {
        let box_stream = Box::new(
            "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            accept:text/html\r\n\r\n\
        "
            .as_bytes(),
        );
        let chunk_body_bytes = "";
        let mut chunk = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Chunk,
            box_stream,
            chunk_body_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 32];
        // Read body part
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 5);

        let box_stream = Box::new("".as_bytes());
        let chunk_body_no_trailer_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            0\r\n\r\n\
            ";

        let mut chunk = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Chunk,
            box_stream,
            chunk_body_no_trailer_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 32];
        // Read body part
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf[..read], b"hello");
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 12);
        assert_eq!(&buf[..read], b"hello world!");
        let read = async_impl::Body::data(&mut chunk, &mut buf).await.unwrap();
        assert_eq!(read, 0);
        assert_eq!(&buf[..read], b"");
        let res = async_impl::Body::trailer(&mut chunk).await.unwrap();
        assert!(res.is_none());
    }

    /// UT test cases for `Body::data`.
    ///
    /// # Brief
    /// 1. Creates a empty `HttpBody`.
    /// 2. Calls `HttpBody::new` to create empty http body.
    /// 3. Checks if http body is empty.
    #[test]
    fn http_body_empty_err() {
        let box_stream = Box::new("".as_bytes());
        let content_bytes = "hello";

        match HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Empty,
            box_stream,
            content_bytes.as_bytes(),
        ) {
            Ok(_) => (),
            Err(e) => assert_eq!(e.error_kind(), ErrorKind::Request),
        }
    }

    /// UT test cases for text `HttpBody::new`.
    ///
    /// # Brief
    /// 1. Creates a text `HttpBody`.
    /// 2. Calls `HttpBody::new` to create text http body.
    /// 3. Checks if result is correct.
    #[test]
    fn ut_http_body_text() {
        let handle = ylong_runtime::spawn(async move {
            http_body_text().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn http_body_text() {
        let box_stream = Box::new("hello world".as_bytes());
        let content_bytes = "";

        let mut text = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Length(11),
            box_stream,
            content_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 5];
        // Read body part
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 1);
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 0);

        let box_stream = Box::new("".as_bytes());
        let content_bytes = "hello";

        let mut text = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::Length(5),
            box_stream,
            content_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 32];
        // Read body part
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut text, &mut buf).await.unwrap();
        assert_eq!(read, 0);
    }

    /// UT test cases for until_close `HttpBody::new`.
    ///
    /// # Brief
    /// 1. Creates a until_close `HttpBody`.
    /// 2. Calls `HttpBody::new` to create until_close http body.
    /// 3. Checks if result is correct.
    #[test]
    fn ut_http_body_until_close() {
        let handle = ylong_runtime::spawn(async move {
            http_body_until_close().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn http_body_until_close() {
        let box_stream = Box::new("hello world".as_bytes());
        let content_bytes = "";

        let mut until_close = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::UntilClose,
            box_stream,
            content_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 5];
        // Read body part
        let read = async_impl::Body::data(&mut until_close, &mut buf)
            .await
            .unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut until_close, &mut buf)
            .await
            .unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut until_close, &mut buf)
            .await
            .unwrap();
        assert_eq!(read, 1);

        let box_stream = Box::new("".as_bytes());
        let content_bytes = "hello";

        let mut until_close = HttpBody::new(
            Arc::new(IdleInterceptor),
            BodyLength::UntilClose,
            box_stream,
            content_bytes.as_bytes(),
        )
        .unwrap();

        let mut buf = [0u8; 5];
        // Read body part
        let read = async_impl::Body::data(&mut until_close, &mut buf)
            .await
            .unwrap();
        assert_eq!(read, 5);
        let read = async_impl::Body::data(&mut until_close, &mut buf)
            .await
            .unwrap();
        assert_eq!(read, 0);
    }
}
