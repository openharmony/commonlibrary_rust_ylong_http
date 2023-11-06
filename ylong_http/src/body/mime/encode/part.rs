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

use core::convert::TryFrom;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io::Read;

use crate::body::mime::common::{data_copy, SizeResult, TokenStatus};
use crate::body::mime::{EncodeHeaders, MixFrom, PartStatus};
use crate::body::{async_impl, sync_impl, MimePart};
use crate::error::{ErrorKind, HttpError};
use crate::{AsyncRead, ReadBuf};

/// `MimePartEncoder` can get a [`MimePart`] to encode into data that can be
/// transmitted.
///
/// [`MimePart`]: MimePart
///
/// # Examples
///
/// ```
/// use ylong_http::body::{sync_impl, MimePart, MimePartEncoder};
///
/// let part = MimePart::builder()
///     .header("accept", "text/html")
///     .body_from_reader("01234567890123456789".as_bytes())
///     .body_from_bytes(b"9876543210\r\n")
///     .build()
///     .unwrap();
/// let mut part_encoder = MimePartEncoder::from_part(part);
/// let mut buf = vec![0u8; 10];
/// let mut v_size = vec![];
/// let mut v_str = vec![];
///
/// loop {
///     let len = sync_impl::Body::data(&mut part_encoder, &mut buf).unwrap();
///     if len == 0 {
///         break;
///     }
///     v_size.push(len);
///     v_str.extend_from_slice(&buf[..len]);
/// }
/// assert_eq!(v_size, vec![10, 10, 10, 2]);
/// assert_eq!(v_str, b"accept:text/html\r\n\r\n9876543210\r\n");
/// ```
#[derive(Debug)]
pub struct MimePartEncoder<'a> {
    // Encode stage now
    stage: PartStatus,
    headers_encode: EncodeHeaders,
    body: Option<MixFrom<'a>>,
    // src which is need to encode
    src: Vec<u8>,
    // index of src
    src_idx: usize,
}

impl<'a> MimePartEncoder<'a> {
    /// Creates a `MimePartEncoder` by a [`MimePart`].
    ///
    /// [`MimePart`]: MimePart
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimePart, MimePartEncoder};
    ///
    /// let part = MimePart::builder().build().unwrap();
    /// let part_encoder = MimePartEncoder::from_part(part);
    /// ```
    pub fn from_part(part: MimePart<'a>) -> Self {
        let body = (!part.body.is_empty()).then_some(part.body);

        MimePartEncoder {
            stage: PartStatus::Start,
            headers_encode: EncodeHeaders::new(part.headers),
            src: vec![],
            src_idx: 0,
            body,
        }
    }

    fn check_next(&mut self) {
        match self.stage {
            PartStatus::Start => {
                self.stage = PartStatus::Headers;
                // init
                self.src_idx = 0;
                self.src = vec![];
            }
            PartStatus::Headers => {
                self.stage = PartStatus::Crlf;
                if self.body.is_some() {
                    // has body, so has Crlf
                    self.src = b"\r\n".to_vec();
                } else {
                    self.src = vec![];
                }
                self.src_idx = 0;
            }
            PartStatus::Crlf => {
                self.stage = PartStatus::Body;
                // Just record index
                self.src_idx = 0;
            }
            PartStatus::Body => {
                self.stage = PartStatus::End;
                self.src_idx = 0;
            }
            PartStatus::End => {
                // init
                self.src_idx = 0;
                self.src = vec![];
            }
        }
    }

    fn start_encode(&mut self) -> SizeResult {
        self.check_next();
        Ok(0)
    }

    // use EncodeHeaders.encode
    fn headers_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        match self.headers_encode.encode(dst)? {
            TokenStatus::Partial(size) => Ok(size),
            TokenStatus::Complete(size) => {
                self.check_next();
                Ok(size)
            }
        }
    }

    fn crlf_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        match data_copy(&self.src, &mut self.src_idx, dst)? {
            TokenStatus::Partial(size) => Ok(size),
            TokenStatus::Complete(size) => {
                self.check_next();
                Ok(size)
            }
        }
    }

    // Uses `syn::Body::data` of `MixFrom`.
    fn body_sync_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        if let Some(body) = &mut self.body {
            let size = sync_impl::Body::data(body, dst)?;
            if size == 0 {
                self.check_next();
            }
            return Ok(size);
        }
        self.check_next();
        Ok(0)
    }
}

impl sync_impl::Body for MimePartEncoder<'_> {
    type Error = std::io::Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut count = 0;
        while count != buf.len() {
            let encode_size = match self.stage {
                PartStatus::Start => self.start_encode(),
                PartStatus::Headers => self.headers_encode(&mut buf[count..]),
                PartStatus::Crlf => self.crlf_encode(&mut buf[count..]),
                PartStatus::Body => self.body_sync_encode(&mut buf[count..]),
                PartStatus::End => return Ok(count),
            };
            count += encode_size?;
        }
        Ok(count)
    }
}

impl async_impl::Body for MimePartEncoder<'_> {
    type Error = std::io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        let mut count = 0;
        while count != buf.len() {
            let encode_size: Poll<SizeResult> = match self.stage {
                PartStatus::Start => Poll::Ready(self.start_encode()),
                PartStatus::Headers => Poll::Ready(self.headers_encode(&mut buf[count..])),
                PartStatus::Crlf => Poll::Ready(self.crlf_encode(&mut buf[count..])),
                PartStatus::Body => match &mut self.body {
                    Some(body) => {
                        let poll_result = Pin::new(body).poll_data(cx, &mut buf[count..]);
                        if let Poll::Ready(Ok(0)) = poll_result {
                            // complete async read body
                            self.check_next();
                        };
                        poll_result
                    }
                    _ => {
                        self.check_next();
                        Poll::Ready(Ok(0))
                    }
                },
                PartStatus::End => return Poll::Ready(Ok(count)),
            };

            match encode_size {
                Poll::Ready(Ok(size)) => count += size,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(count))
    }
}

#[cfg(test)]
mod ut_mime_part_encoder {
    use crate::body::{async_impl, sync_impl, MimePart, MimePartEncoder};
    use crate::headers::Headers;

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets body by `body_from_owned`.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_body_from_owned() {
        part_encode_compare! (
            MimePart: {
                BodyOwned: b"123456".to_vec(),
            },
            ResultData: b"\r\n123456",
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets body by `body_from_bytes`.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_body_from_bytes() {
        part_encode_compare! (
            MimePart: {
                BodySlice: "123456".as_bytes(),
            },
            BufSize: 5,
            ResultData: b"\r\n123456",
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets body by `body_from_reader`.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_body_from_reader() {
        part_encode_compare! (
            MimePart: {
                BodyReader: "123456".as_bytes(),
            },
            BufSize: 5,
            ResultData: b"\r\n123456",
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets headers and sets body.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_data_common() {
        part_encode_compare! (
            MimePart: {
                Header: "accept", "text/html",
                BodyReader: "9876543210\r\n".as_bytes(),
            },
            BufSize: 10,
            ResultSize: vec![10, 10, 10, 2],
            ResultData: b"accept:text/html\r\n\r\n9876543210\r\n",
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Doesn't set headers and only sets body.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_data_noheaders() {
        part_encode_compare! (
            MimePart: {
                BodySlice: b"0123456789--0123",
            },
            BufSize: 10,
            ResultSize: vec![10, 8],
            ResultData: b"\r\n0123456789--0123",
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Doesn't set body and only sets headers.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_data_nobody() {
        // no body no CRLF
        part_encode_compare! (
            MimePart: {
                Header: "key", "\"value\"",
            },
            ResultData: b"key:\"value\"\r\n",
            Sync,
        );

        part_encode_compare! (
            MimePart: {
                Header: "key1", "value1",
                Header: "key2", "value2",
            },
            BufSize: 10,
            ResultSize: vec![10, 10, 6],
            Sync,
        );
    }

    /// UT test cases for `syn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Doesn't set headers and doesn't set headers.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_encoder_data_noheaders_nobody() {
        part_encode_compare! (
            MimePart: {
            },
            ResultData: b"",
            Sync,
        );
    }

    /// UT test cases for `asyn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets body by `body_from_owned`.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes asynchronously.
    /// 4. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mime_part_encoder_body_from_owned_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mime_part_encoder_body_from_owned_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mime_part_encoder_body_from_owned_then_async_data() {
        part_encode_compare! (
            MimePart: {
                BodyOwned: b"123456".to_vec(),
            },
            BufSize: 5,
            ResultSize: vec![5, 3],
            ResultData: b"\r\n123456",
            Async,
        );
    }

    /// UT test cases for `asyn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets body by `body_from_bytes`.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes asynchronously.
    /// 4. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mime_part_encoder_body_from_bytes_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mime_part_encoder_body_from_bytes_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mime_part_encoder_body_from_bytes_then_async_data() {
        part_encode_compare! (
            MimePart: {
                BodySlice: b"123456",
            },
            BufSize: 5,
            ResultSize: vec![5, 3],
            ResultData: b"\r\n123456",
            Async,
        );
    }

    /// UT test cases for `asyn::Body::data` of `MimePartEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePart`.
    /// 2. Sets headers and sets body.
    /// 3. Builds a `MimePartEncoder` by `from_part` and encodes asynchronously.
    /// 4. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mime_part_encoder_common_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mime_part_encoder_common_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mime_part_encoder_common_then_async_data() {
        part_encode_compare! (
            MimePart: {
                Header: "accept", "text/html",
                BodySlice: b"9876543210\r\n",
            },
            BufSize: 10,
            ResultSize: vec![10, 10, 10, 2],
            ResultData: b"accept:text/html\r\n\r\n9876543210\r\n",
            Async,
        );
    }
}
