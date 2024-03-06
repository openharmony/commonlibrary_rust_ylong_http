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

use core::cmp::min;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io::{Error, Read};

use super::origin::{FromAsyncReader, FromBytes, FromReader};
use super::{async_impl, sync_impl};
use crate::body::origin::FromAsyncBody;
use crate::{AsyncRead, ReadBuf};

/// `TextBody` is used to represent the body of plain text type.
///
/// You can create a `TextBody` in a variety of ways, such as reading from
/// memory or reading from a file.
///
/// # Read From Memory
///
/// You can create a `TextBody` by reading memory slice.
///
/// For example, you can use a memory slice to create a `TextBody`:
///
/// ```
/// use ylong_http::body::TextBody;
///
/// let text = "Hello World";
/// let body = TextBody::from_bytes(text.as_bytes());
/// ```
///
/// This type of `TextBody` implements both [`sync_impl::Body`] and
/// [`async_impl::Body`].
///
/// # Read From Reader
///
/// You can create a `TextBody` by reading from a synchronous reader.
///
/// For example, you can use a `&[u8]` to create a `TextBody`:
///
/// ```no_run
/// use ylong_http::body::TextBody;
///
/// // In this usage `&[u8]` is treated as a synchronous reader.
/// let reader = "Hello World";
/// let body = TextBody::from_reader(reader.as_bytes());
/// ```
///
/// This type of `TextBody` **only** implements [`sync_impl::Body`].
///
/// # Read From Async Reader
///
/// You can create a `TextBody` by reading from an asynchronous reader.
///
/// For example, you can use a `&[u8]` to create a `TextBody`:
///
/// ```no_run
/// use ylong_http::body::TextBody;
///
/// async fn text_body_from_async_reader() {
///     // In this usage `&[u8]` is treated as an asynchronous reader.
///     let reader = "Hello World";
///     let body = TextBody::from_async_reader(reader.as_bytes());
/// }
/// ```
///
/// This type of `TextBody` **only** implements [`async_impl::Body`].
///
/// # Read Body Content
///
/// After you have created a `TextBody`, you can use the methods of
/// [`sync_impl::Body`] or [`async_impl::Body`] to read data, like the examples
/// below:
///
/// sync:
///
/// ```no_run
/// use ylong_http::body::sync_impl::Body;
/// use ylong_http::body::TextBody;
///
/// let text = "Hello World";
/// let mut body = TextBody::from_bytes(text.as_bytes());
///
/// let mut buf = [0u8; 1024];
/// loop {
///     let size = body.data(&mut buf).unwrap();
///     if size == 0 {
///         break;
///     }
///     // Operates on the data you read..
/// }
/// ```
///
/// async:
///
/// ```no_run
/// use ylong_http::body::async_impl::Body;
/// use ylong_http::body::TextBody;
///
/// async fn read_from_body() {
///     let text = "Hello World";
///     let mut body = TextBody::from_bytes(text.as_bytes());
///
///     let mut buf = [0u8; 1024];
///     loop {
///         let size = body.data(&mut buf).await.unwrap();
///         if size == 0 {
///             break;
///         }
///         // Operates on the data you read..
///     }
/// }
/// ```
///
/// [`sync_impl::Body`]: sync_impl::Body
/// [`async_impl::Body`]: async_impl::Body
pub struct TextBody<T> {
    from: T,
}

impl<'a> TextBody<FromBytes<'a>> {
    /// Creates a `TextBody` by a memory slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBody;
    ///
    /// let text = "Hello World";
    /// let body = TextBody::from_bytes(text.as_bytes());
    /// ```
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        TextBody {
            from: FromBytes::new(bytes),
        }
    }
}

impl<T: Read> TextBody<FromReader<T>> {
    /// Creates a `TextBody` from a synchronous reader.
    ///
    /// ```no_run
    /// use ylong_http::body::TextBody;
    ///
    /// // In this usage `&[u8]` is treated as a synchronous reader.
    /// let reader = "Hello World";
    /// let body = TextBody::from_reader(reader.as_bytes());
    /// ```
    pub fn from_reader(reader: T) -> Self {
        TextBody {
            from: FromReader::new(reader),
        }
    }
}

impl<T: AsyncRead + Unpin + Send + Sync> TextBody<FromAsyncReader<T>> {
    /// Creates a `TextBody` from an asynchronous reader.
    ///
    /// ```no_run
    /// use ylong_http::body::TextBody;
    ///
    /// async fn text_body_from_async_reader() {
    ///     let reader = "Hello World";
    ///     let body = TextBody::from_async_reader(reader.as_bytes());
    /// }
    /// ```
    pub fn from_async_reader(reader: T) -> Self {
        Self {
            from: FromAsyncReader::new(reader),
        }
    }
}

impl<'a> sync_impl::Body for TextBody<FromBytes<'a>> {
    type Error = Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Read::read(&mut *self.from, buf)
    }
}

impl<'c> async_impl::Body for TextBody<FromBytes<'c>> {
    type Error = Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        Poll::Ready(Read::read(&mut *self.from, buf))
    }
}

impl<T: Read> sync_impl::Body for TextBody<FromReader<T>> {
    type Error = Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.from.read(buf)
    }
}

impl<T: AsyncRead + Unpin + Send + Sync> async_impl::Body for TextBody<FromAsyncReader<T>> {
    type Error = Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        let mut buf = ReadBuf::new(buf);
        match Pin::new(&mut *self.from).poll_read(cx, &mut buf) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.filled().len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A decoder for decoding plaintext body.
///
/// You need to provide the decoder with a body length and some byte slices
/// containing a legal body. The decoder will divide the correct body and
/// redundant parts according to the `HTTP` syntax.
///
/// This decoder supports decoding segmented byte slices.
///
/// # Examples
///
/// ```
/// use ylong_http::body::TextBodyDecoder;
///
/// // Creates a decoder and set the body length to 20.
/// let mut decoder = TextBodyDecoder::new(20);
///
/// // Provides the decoder with the first slice that may contain the body data.
/// // The length of this slice is 10, which is less than 20, so it is considered
/// // legal body data.
/// // The remaining body length is 10 after decoding.
/// let slice1 = b"This is a ";
/// let (text, left) = decoder.decode(slice1);
/// // Since the slice provided before is not enough for the decoder to
/// // complete the decoding, the status of the returned `Text` is `Partial`
/// // and no left data is returned.
/// assert!(text.is_partial());
/// assert_eq!(text.data(), b"This is a ");
/// assert!(left.is_empty());
///
/// // Provides the decoder with the second slice that may contain the body data.
/// // The data length is 26, which is more than 10, so the first 10 bytes of
/// // the data will be considered legal body, and the rest will be considered
/// // redundant data.
/// let slice2 = b"text body.[REDUNDANT DATA]";
/// let (text, left) = decoder.decode(slice2);
/// // Since the body data is fully decoded, the status of the returned `Text`
/// // is `Complete`. The left data is also returned.
/// assert!(text.is_complete());
/// assert_eq!(text.data(), b"text body.");
/// assert_eq!(left, b"[REDUNDANT DATA]");
///
/// // Provides the decoder with the third slice. Since the body data has been
/// // fully decoded, the given slice is regard as redundant data.
/// let slice3 = b"[REDUNDANT DATA]";
/// let (text, left) = decoder.decode(slice3);
/// assert!(text.is_complete());
/// assert!(text.data().is_empty());
/// assert_eq!(left, b"[REDUNDANT DATA]");
/// ```
pub struct TextBodyDecoder {
    left: usize,
}

impl TextBodyDecoder {
    /// Creates a new `TextBodyDecoder` from a body length.
    ///
    /// This body length generally comes from the `Content-Length` field.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBodyDecoder;
    ///
    /// let decoder = TextBodyDecoder::new(10);
    /// ```
    pub fn new(length: usize) -> TextBodyDecoder {
        TextBodyDecoder { left: length }
    }

    /// Decodes a byte slice that may contain a plaintext body. This method
    /// supports decoding segmented byte slices.
    ///
    /// After each call to this method, a `Text` and a `&[u8]` are returned.
    /// `Text` contains a piece of legal body data inside. The returned `&[u8]`
    /// contains redundant data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBodyDecoder;
    ///
    /// // Creates a decoder and set the body length to 20.
    /// let mut decoder = TextBodyDecoder::new(20);
    ///
    /// // Provides the decoder with the first slice that may contain the body data.
    /// // The length of this slice is 10, which is less than 20, so it is considered
    /// // legal body data.
    /// // The remaining body length is 10 after decoding.
    /// let slice1 = b"This is a ";
    /// let (text, left) = decoder.decode(slice1);
    /// // Since the slice provided before is not enough for the decoder to
    /// // complete the decoding, the status of the returned `Text` is `Partial`
    /// // and no left data is returned.
    /// assert!(text.is_partial());
    /// assert_eq!(text.data(), b"This is a ");
    /// assert!(left.is_empty());
    ///
    /// // Provides the decoder with the second slice that may contain the body data.
    /// // The data length is 26, which is more than 10, so the first 10 bytes of
    /// // the data will be considered legal body, and the rest will be considered
    /// // redundant data.
    /// let slice2 = b"text body.[REDUNDANT DATA]";
    /// let (text, left) = decoder.decode(slice2);
    /// // Since the body data is fully decoded, the status of the returned `Text`
    /// // is `Complete`. The left data is also returned.
    /// assert!(text.is_complete());
    /// assert_eq!(text.data(), b"text body.");
    /// assert_eq!(left, b"[REDUNDANT DATA]");
    ///
    /// // Provides the decoder with the third slice. Since the body data has been
    /// // fully decoded, the given slice is regard as redundant data.
    /// let slice3 = b"[REDUNDANT DATA]";
    /// let (text, left) = decoder.decode(slice3);
    /// assert!(text.is_complete());
    /// assert!(text.data().is_empty());
    /// assert_eq!(left, b"[REDUNDANT DATA]");
    /// ```
    pub fn decode<'a>(&mut self, buf: &'a [u8]) -> (Text<'a>, &'a [u8]) {
        if self.left == 0 {
            return (Text::complete(&buf[..0]), buf);
        }

        let size = min(self.left, buf.len());
        self.left -= size;
        if self.left == 0 {
            (Text::complete(&buf[..size]), &buf[size..])
        } else {
            (Text::partial(&buf[..size]), &buf[size..])
        }
    }
}

/// Decode result of a text buffer.
/// The `state` records the decode status, and the data records the decoded
/// data.
#[derive(Debug)]
pub struct Text<'a> {
    state: TextState,
    data: &'a [u8],
}

impl<'a> Text<'a> {
    /// Checks whether this `Text` contains the last valid part of the body
    /// data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBodyDecoder;
    ///
    /// let bytes = b"This is a ";
    /// let mut decoder = TextBodyDecoder::new(20);
    /// let (text, _) = decoder.decode(bytes);
    /// assert!(!text.is_complete());
    ///
    /// let bytes = b"text body.";
    /// let (text, _) = decoder.decode(bytes);
    /// assert!(text.is_complete());
    /// ```
    pub fn is_complete(&self) -> bool {
        matches!(self.state, TextState::Complete)
    }

    /// Checks whether this `Text` contains a non-last part of the body data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBodyDecoder;
    ///
    /// let bytes = b"This is a ";
    /// let mut decoder = TextBodyDecoder::new(20);
    /// let (text, _) = decoder.decode(bytes);
    /// assert!(text.is_partial());
    ///
    /// let bytes = b"text body.";
    /// let (text, _) = decoder.decode(bytes);
    /// assert!(!text.is_partial());
    /// ```
    pub fn is_partial(&self) -> bool {
        !self.is_complete()
    }

    /// Gets the underlying data of this `Text`. The returned data is a part
    /// of the body data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::TextBodyDecoder;
    ///
    /// let bytes = b"This is a text body.";
    /// let mut decoder = TextBodyDecoder::new(20);
    /// let (text, _) = decoder.decode(bytes);
    /// assert_eq!(text.data(), b"This is a text body.");
    /// ```
    pub fn data(&self) -> &[u8] {
        self.data
    }

    pub(crate) fn complete(data: &'a [u8]) -> Self {
        Self {
            state: TextState::Complete,
            data,
        }
    }

    pub(crate) fn partial(data: &'a [u8]) -> Self {
        Self {
            state: TextState::Partial,
            data,
        }
    }
}

#[derive(Debug)]
enum TextState {
    Partial,
    Complete,
}

#[cfg(test)]
mod ut_text {
    use crate::body::text::{TextBody, TextBodyDecoder};

    /// UT test cases for `TextBody::from_bytes`.
    ///
    /// # Brief
    /// 1. Calls `TextBody::from_bytes()` to create a `TextBody`.
    #[test]
    fn ut_text_body_from_bytes() {
        let bytes = b"Hello World!";
        let _body = TextBody::from_bytes(bytes);
        // Success if no panic.
    }

    /// UT test cases for `TextBody::from_reader`.
    ///
    /// # Brief
    /// 1. Calls `TextBody::from_reader()` to create a `TextBody`.
    #[test]
    fn ut_text_body_from_reader() {
        let reader = "Hello World!".as_bytes();
        let _body = TextBody::from_reader(reader);
        // Success if no panic.
    }

    /// UT test cases for `TextBody::from_async_reader`.
    ///
    /// # Brief
    /// 1. Calls `TextBody::from_async_reader()` to create a `TextBody`.
    #[test]
    fn ut_text_body_from_async_reader() {
        let reader = "Hello World!".as_bytes();
        let _body = TextBody::from_async_reader(reader);
        // Success if no panic.
    }

    /// UT test cases for `sync_impl::Body::data` of `TextBody<FromBytes<'_>>`.
    ///
    /// # Brief
    /// 1. Creates a `TextBody<FromBytes<'_>>`.
    /// 2. Calls its `sync_impl::Body::data` method and then checks the results.
    #[test]
    fn ut_text_body_from_bytes_syn_data() {
        use crate::body::sync_impl::Body;

        let bytes = b"Hello World!";
        let mut body = TextBody::from_bytes(bytes);
        let mut buf = [0u8; 5];

        let size = body.data(&mut buf).expect("First read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b"Hello");

        let size = body.data(&mut buf).expect("Second read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b" Worl");

        let size = body.data(&mut buf).expect("Third read failed.");
        assert_eq!(size, 2);
        assert_eq!(&buf[..size], b"d!");
    }

    /// UT test cases for `async_impl::Body::data` of `TextBody<FromBytes<'_>>`.
    ///
    /// # Brief
    /// 1. Creates a `TextBody<FromBytes<'_>>`.
    /// 2. Calls its `async_impl::Body::data` method and then checks the
    ///    results.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_text_body_from_bytes_asyn_data() {
        let handle = ylong_runtime::spawn(async move {
            text_body_from_bytes_asyn_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn text_body_from_bytes_asyn_data() {
        use crate::body::async_impl::Body;

        let bytes = b"Hello World!";
        let mut body = TextBody::from_bytes(bytes);
        let mut buf = [0u8; 5];

        let size = body.data(&mut buf).await.expect("First read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b"Hello");

        let size = body.data(&mut buf).await.expect("Second read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b" Worl");

        let size = body.data(&mut buf).await.expect("Third read failed.");
        assert_eq!(size, 2);
        assert_eq!(&buf[..size], b"d!");
    }

    /// UT test cases for `sync_impl::Body::data` of `TextBody<FromReader<T>>`.
    ///
    /// # Brief
    /// 1. Creates a `TextBody<FromReader<T>>`.
    /// 2. Calls its `sync_impl::Body::data` method and then checks the results.
    #[test]
    fn ut_text_body_from_reader_syn_data() {
        use crate::body::sync_impl::Body;

        let reader = "Hello World!".as_bytes();
        let mut body = TextBody::from_reader(reader);
        let mut buf = [0u8; 5];

        let size = body.data(&mut buf).expect("First read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b"Hello");

        let size = body.data(&mut buf).expect("Second read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b" Worl");

        let size = body.data(&mut buf).expect("Third read failed.");
        assert_eq!(size, 2);
        assert_eq!(&buf[..size], b"d!");
    }

    /// UT test cases for `async_impl::Body::data` of
    /// `TextBody<FromAsyncReader<T>>`.
    ///
    /// # Brief
    /// 1. Creates a `TextBody<FromAsyncReader<T>>`.
    /// 2. Calls its `async_impl::Body::data` method and then checks the
    ///    results.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_text_body_from_async_reader_asyn_data() {
        let handle = ylong_runtime::spawn(async move {
            text_body_from_async_reader_asyn_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn text_body_from_async_reader_asyn_data() {
        use crate::body::async_impl::Body;

        let reader = "Hello World!".as_bytes();
        let mut body = TextBody::from_async_reader(reader);
        let mut buf = [0u8; 5];

        let size = body.data(&mut buf).await.expect("First read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b"Hello");

        let size = body.data(&mut buf).await.expect("Second read failed.");
        assert_eq!(size, 5);
        assert_eq!(&buf[..size], b" Worl");

        let size = body.data(&mut buf).await.expect("Third read failed.");
        assert_eq!(size, 2);
        assert_eq!(&buf[..size], b"d!");
    }

    /// UT test cases for `TextBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `TextBodyDecoder` by calling `TextBodyDecoder::new`.
    /// 2. Decodes text body by calling `TextBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_text_body_decoder_decode() {
        // Test 1:
        let bytes = b"this is the text body! and this is remaining data";
        let mut decoder = TextBodyDecoder::new(22);
        let (text, left) = decoder.decode(&bytes[..4]);
        assert!(text.is_partial());
        assert_eq!(text.data(), b"this");
        assert!(left.is_empty());

        let (text, left) = decoder.decode(&bytes[4..11]);
        assert!(text.is_partial());
        assert_eq!(text.data(), b" is the");
        assert!(left.is_empty());

        let (text, left) = decoder.decode(&bytes[11..26]);
        assert!(text.is_complete());
        assert_eq!(text.data(), b" text body!");
        assert_eq!(left, b" and");

        let (text, left) = decoder.decode(&bytes[26..]);
        assert!(text.is_complete());
        assert!(text.data().is_empty());
        assert_eq!(left, b" this is remaining data");

        // Test 2:
        let bytes = b"this is the text body! And this is remaining data";
        let mut decoder = TextBodyDecoder::new(22);
        let (text, left) = decoder.decode(bytes);
        assert!(text.is_complete());
        assert_eq!(text.data(), b"this is the text body!");
        assert_eq!(left, b" And this is remaining data");
    }
}
