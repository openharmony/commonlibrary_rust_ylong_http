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
use std::collections::VecDeque;

use crate::body::mime::common::{data_copy, SizeResult};
use crate::body::mime::EncodeHeaders;
use crate::body::{async_impl, sync_impl, MimeMulti, MimePartEncoder, TokenStatus, XPart};
use crate::headers::Headers;

/// `MimeMultiEncoder` can get a [`MimeMulti`] to encode into data that can be
/// transmitted.
///
/// [`MimeMulti`]: MimeMulti
///
/// # Examples
///
/// ```
/// use ylong_http::body::{sync_impl, MimeMulti, MimeMultiEncoder, MimePart};
///
/// let multi1 = MimeMulti::builder()
///     .set_content_type(b"multipart/mixed", b"abc".to_vec())
///     .set_boundary(b"abc".to_vec())
///     .add_part(
///         MimePart::builder()
///             .header("key1", "value1")
///             .body_from_reader("111111".as_bytes())
///             .build()
///             .unwrap(),
///     )
///     .add_part(
///         MimePart::builder()
///             .header("key2", "value2")
///             .body_from_reader("22222".as_bytes())
///             .build()
///             .unwrap(),
///     )
///     .build()
///     .unwrap();
///
/// let multi = MimeMulti::builder()
///     .set_boundary(b"abcde".to_vec())
///     .add_part(
///         MimePart::builder()
///             .header("key3", "value3")
///             .body_from_reader("33333".as_bytes())
///             .build()
///             .unwrap(),
///     )
///     .add_multi(multi1)
///     .add_part(
///         MimePart::builder()
///             .header("key4", "value4")
///             .body_from_async_reader("444444".as_bytes()) // nothing in sync
///             .build()
///             .unwrap(),
///     )
///     .build()
///     .unwrap();
///
/// let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
/// let mut buf = vec![0u8; 30];
/// let mut v_size = vec![];
/// let mut v_str = vec![];
///
/// loop {
///     let len = sync_impl::Body::data(&mut multi_encoder, &mut buf).unwrap();
///     if len == 0 {
///         break;
///     }
///     v_size.push(len);
///     v_str.extend_from_slice(&buf[..len]);
/// }
/// assert_eq!(v_size, vec![30, 30, 30, 30, 30, 30, 13]);
/// assert_eq!(
///     v_str,
///     b"--abcde\r\nkey3:value3\r\n\r\n33333\r\n--abcde\r\n\
///     content-type:multipart/mixed; boundary=abc\r\n\r\n--abc\r\n\
///     key1:value1\r\n\r\n111111\r\n--abc\r\nkey2:value2\r\n\r\n22222\r\n\
///     --abc--\r\n\r\n--abcde\r\nkey4:value4\r\n\r\n\r\n--abcde--\r\n"
/// );
/// ```
#[derive(Debug)]
pub struct MimeMultiEncoder<'a> {
    stages: VecDeque<MultiStage<'a>>,
    // default is not part, don't encode headers
    headers: Option<Headers>,
    src_idx: usize,
    src: Vec<u8>,
}

impl<'a> MimeMultiEncoder<'a> {
    /// Creates a `MimeMultiEncoder` by a [`MimeMulti`].
    ///
    /// [`MimeMulti`]: MimeMulti
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimeMulti, MimeMultiEncoder};
    ///
    /// let multi = MimeMulti::builder().build().unwrap();
    /// let multi_encoder = MimeMultiEncoder::from_multi(multi);
    /// ```
    pub fn from_multi(multi: MimeMulti<'a>) -> Self {
        let mut encoder = Self::new();
        encoder.headers = Some(multi.headers);
        encoder.push_list_to_stages(multi.boundary, multi.list);
        encoder
    }

    fn new() -> Self {
        MimeMultiEncoder {
            stages: VecDeque::new(),
            headers: None,
            src: vec![],
            src_idx: 0,
        }
    }

    fn push_list_to_stages(&mut self, boundary: Vec<u8>, list: Vec<XPart<'a>>) {
        // dash-boundary := "--" boundary
        self.stages.push_back(MultiStage::Dash);
        self.stages
            .push_back(MultiStage::Boundary(boundary.clone()));
        self.stages.push_back(MultiStage::Crlf);

        if list.is_empty() {
            // close-delimiter  CRLF "--" boundary "--" CRLF
            self.push_end_boundary(boundary);
            return;
        }

        let len = list.len();

        for (idx, xpart) in list.into_iter().enumerate() {
            match xpart {
                XPart::Part(part) => {
                    let part_encoder = MimePartEncoder::from_part(part);
                    self.stages.push_back(MultiStage::MimePart(part_encoder));
                }
                XPart::Multi(multi) => self.push_multi_as_part(multi),
            }
            if idx == len - 1 {
                self.push_end_boundary(boundary.clone());
            } else {
                self.push_middle_boundary(boundary.clone());
            }
        }
    }

    fn push_multi_as_part(&mut self, multi: MimeMulti<'a>) {
        let headers_encoder = EncodeHeaders::new(multi.headers);
        self.stages.push_back(MultiStage::Headers(headers_encoder));
        self.stages.push_back(MultiStage::Crlf);
        self.push_list_to_stages(multi.boundary, multi.list);
    }

    // "--" boundary CRLF
    fn push_first_boundary(&mut self, boundary: Vec<u8>) {
        // dash-boundary := "--" boundary
        self.stages.push_back(MultiStage::Dash);
        self.stages.push_back(MultiStage::Boundary(boundary));
        // end CRLF
        self.stages.push_back(MultiStage::Crlf);
    }

    // CRLF "--" boundary CRLF
    fn push_middle_boundary(&mut self, boundary: Vec<u8>) {
        // delimiter := CRLF dash-boundary
        self.stages.push_back(MultiStage::Crlf);
        self.push_first_boundary(boundary);
    }

    // CRLF "--" boundary "--" CRLF
    fn push_end_boundary(&mut self, boundary: Vec<u8>) {
        // close-delimiter: = CRLF "--" boundary "--"
        self.stages.push_back(MultiStage::Crlf);
        self.stages.push_back(MultiStage::Dash);
        self.stages.push_back(MultiStage::Boundary(boundary));
        self.stages.push_back(MultiStage::Dash);
        // end CRLF
        self.stages.push_back(MultiStage::Crlf);
    }

    fn init_src(&mut self) {
        self.src = vec![];
        self.src_idx = 0;
    }

    fn temp_src(&mut self, stage: &MultiStage) {
        match stage {
            MultiStage::Crlf => self.src = b"\r\n".to_vec(),
            MultiStage::Dash => self.src = b"--".to_vec(),
            _ => {}
        }
    }

    //"\r\n"
    fn crlf_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        match data_copy(&self.src, &mut self.src_idx, dst)? {
            TokenStatus::Partial(size) => {
                self.stages.push_front(MultiStage::Crlf);
                Ok(size)
            }
            TokenStatus::Complete(size) => {
                self.init_src();
                Ok(size)
            }
        }
    }

    // "--"
    fn dash_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        match data_copy(&self.src, &mut self.src_idx, dst)? {
            TokenStatus::Partial(size) => {
                self.stages.push_front(MultiStage::Dash);
                Ok(size)
            }
            TokenStatus::Complete(size) => {
                self.init_src();
                Ok(size)
            }
        }
    }

    fn boundary_encode(&mut self, dst: &mut [u8], boundary: Vec<u8>) -> SizeResult {
        match data_copy(&boundary, &mut self.src_idx, dst)? {
            TokenStatus::Partial(size) => {
                self.stages.push_front(MultiStage::Boundary(boundary));
                Ok(size)
            }
            TokenStatus::Complete(size) => {
                self.init_src();
                Ok(size)
            }
        }
    }

    fn headers_encode(&mut self, dst: &mut [u8], mut headers_encoder: EncodeHeaders) -> SizeResult {
        match headers_encoder.encode(dst)? {
            TokenStatus::Partial(size) => {
                self.stages.push_front(MultiStage::Headers(headers_encoder));
                Ok(size)
            }
            TokenStatus::Complete(size) => {
                self.init_src();
                Ok(size)
            }
        }
    }

    fn sync_mimepart_encode(
        &mut self,
        dst: &mut [u8],
        mut part_encoder: MimePartEncoder<'a>,
    ) -> SizeResult {
        let size = sync_impl::Body::data(&mut part_encoder, dst)?;
        if size != 0 {
            self.stages.push_front(MultiStage::MimePart(part_encoder));
        } else {
            self.init_src();
        }
        Ok(size)
    }
}

impl sync_impl::Body for MimeMultiEncoder<'_> {
    type Error = std::io::Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut count = 0;
        while count != buf.len() {
            let stage = match self.stages.pop_front() {
                Some(stage) => stage,
                None => break,
            };
            self.temp_src(&stage);
            let size = match stage {
                MultiStage::Crlf => self.crlf_encode(&mut buf[count..]),
                MultiStage::Dash => self.dash_encode(&mut buf[count..]),
                MultiStage::Boundary(boundary) => self.boundary_encode(&mut buf[count..], boundary),
                MultiStage::Headers(headers_encoder) => {
                    self.headers_encode(&mut buf[count..], headers_encoder)
                }
                MultiStage::MimePart(part_encoder) => {
                    self.sync_mimepart_encode(&mut buf[count..], part_encoder)
                }
            }?;
            count += size;
        }
        Ok(count)
    }
}

impl async_impl::Body for MimeMultiEncoder<'_> {
    type Error = std::io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        let mut count = 0;
        while count != buf.len() {
            let stage = match self.stages.pop_front() {
                Some(stage) => stage,
                None => break,
            };
            self.temp_src(&stage);
            let poll_size: Poll<SizeResult> = match stage {
                MultiStage::Crlf => Poll::Ready(self.crlf_encode(&mut buf[count..])),
                MultiStage::Dash => Poll::Ready(self.dash_encode(&mut buf[count..])),
                MultiStage::Boundary(boundary) => {
                    Poll::Ready(self.boundary_encode(&mut buf[count..], boundary))
                }
                MultiStage::Headers(headers_encoder) => {
                    Poll::Ready(self.headers_encode(&mut buf[count..], headers_encoder))
                }
                MultiStage::MimePart(mut part_encoder) => {
                    let poll_result = Pin::new(&mut part_encoder).poll_data(cx, &mut buf[count..]);
                    if let Poll::Ready(Ok(0)) = poll_result {
                        // complete async read body
                        self.init_src();
                    } else if let Poll::Ready(Ok(_)) = poll_result {
                        self.stages.push_front(MultiStage::MimePart(part_encoder));
                    }
                    poll_result
                }
            };

            match poll_size {
                Poll::Ready(Ok(size)) => count += size,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(count))
    }
}

#[derive(Debug)]
enum MultiStage<'a> {
    // \r\n
    Crlf,
    // --
    Dash,
    // boundary
    Boundary(Vec<u8>),
    // headers
    Headers(EncodeHeaders),
    // part
    MimePart(MimePartEncoder<'a>),
}

#[cfg(test)]
mod ut_mime_multi_encoder {
    use crate::body::{async_impl, sync_impl, MimeMulti, MimeMultiEncoder, MimePart};

    // TODO: Uses a commonly macro like `part_encode_compare` to replace this one.
    macro_rules! encode_and_compare {
        ( $buf: expr, $encode: block, $v_size: expr, $v_str: expr) => {
            let mut v_size = vec![];
            let mut v_str = vec![];

            loop {
                let len = $encode;
                if len == 0 {
                    break;
                }
                v_size.push(len);
                v_str.extend_from_slice(&$buf[..len]);
            }
            assert_eq!(v_size, $v_size);
            assert_eq!(v_str, $v_str);
        };
    }

    /// UT test cases for `syn::Body::data` of `MimeMultiEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMulti`.
    /// 2. Builds a `MimeMultiEncoder` by `from_multi` and encodes.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_encoder_from_multi() {
        let multi = MimeMulti::builder().build().unwrap();
        let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
        let mut buf = vec![0u8; 10];
        encode_and_compare!(
            buf,
            { sync_impl::Body::data(&mut multi_encoder, &mut buf).unwrap() },
            vec![10, 4],
            b"---\r\n\r\n-----\r\n"
        );
    }

    /// UT test cases for `syn::Body::data` of `MimeMultiEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMulti`.
    /// 2. Sets headers.
    /// 3. Builds a `MimeMultiEncoder` by `from_multi` and encodes.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_encoder_data_one_part() {
        let part = MimePart::builder()
            .header("key1", "value1")
            .body_from_reader("9876543210\r\n".as_bytes())
            .build()
            .unwrap();
        let multi = MimeMulti::builder().add_part(part).build().unwrap();
        let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
        let mut buf = vec![0u8; 10];
        encode_and_compare!(
            buf,
            { sync_impl::Body::data(&mut multi_encoder, &mut buf).unwrap() },
            vec![10, 10, 10, 10, 1],
            b"---\r\nkey1:value1\r\n\r\n9876543210\r\n\r\n-----\r\n"
        );
    }

    /// UT test cases for `syn::Body::data` of `MimeMultiEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMulti`.
    /// 2. Creates several `MimePart`, sets headers, sets body, builds.
    /// 3. Adds `MimePart` to `MimeMulti` by `add_part`.
    /// 4. Builds a `MimeMultiEncoder` by `from_multi` and encodes
    ///    synchronously.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_encoder_data_many_parts() {
        let multi = MimeMulti::builder()
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_reader("98765432\r\n".as_bytes())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_reader("22222".as_bytes())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
        let mut buf = vec![0u8; 10];
        encode_and_compare!(
            buf,
            { sync_impl::Body::data(&mut multi_encoder, &mut buf).unwrap() },
            vec![10, 10, 10, 10, 10, 10, 6],
            b"---\r\nkey1:value1\r\n\r\n98765432\r\n\r\n---\r\nkey2:value2\r\n\r\n22222\r\n-----\r\n"
        );
    }

    /// UT test cases for `syn::Body::data` of `MimeMultiEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMulti`.
    /// 2. Creates several `MimePart`, sets headers, sets body, builds.
    /// 3. Creates a main `MimeMulti`, adds parts.
    /// 4. Builds a `MimeMultiEncoder` by `from_multi` and encodes
    ///    synchronously.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_encoder_data_many_parts_nesting() {
        let multi1 = MimeMulti::builder()
            .set_content_type(b"multipart/mixed", b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_reader("111111".as_bytes())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_reader("22222".as_bytes())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let multi = MimeMulti::builder()
            .set_boundary(b"abcde".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key3", "value3")
                    .body_from_reader("33333".as_bytes())
                    .build()
                    .unwrap(),
            )
            .add_multi(multi1)
            .add_part(
                MimePart::builder()
                    .header("key4", "value4")
                    .body_from_async_reader("444444".as_bytes()) // nothing in sync
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
        let mut buf = vec![0u8; 30];
        encode_and_compare!(
            buf,
            { sync_impl::Body::data(&mut multi_encoder, &mut buf).unwrap() },
            vec![30, 30, 30, 30, 30, 30, 13],
            b"--abcde\r\nkey3:value3\r\n\r\n33333\r\n--abcde\r\n\
            content-type:multipart/mixed; boundary=abc\r\n\r\n--abc\r\n\
            key1:value1\r\n\r\n111111\r\n--abc\r\nkey2:value2\r\n\r\n22222\r\n\
            --abc--\r\n\r\n--abcde\r\nkey4:value4\r\n\r\n\r\n--abcde--\r\n"
        );
    }

    /// UT test cases for `asyn::Body::data` of `MimeMultiEncoder`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMulti`.
    /// 2. Creates several `MimePart`, sets headers, sets body, builds.
    /// 3. Creates a main `MimeMulti`, adds parts.
    /// 4. Builds a `MimeMultiEncoder` by `from_multi` and encodes
    ///    asynchronously.
    /// 5. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mime_multi_encoder_data_many_parts_nesting_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mime_multi_encoder_data_many_parts_nesting_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mime_multi_encoder_data_many_parts_nesting_then_async_data() {
        let multi1 = MimeMulti::builder()
            .set_content_type(b"multipart/mixed", b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_async_reader("111111".as_bytes())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_async_reader("22222".as_bytes())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let multi = MimeMulti::builder()
            .set_boundary(b"abcde".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key3", "value3")
                    .body_from_bytes("33333".as_bytes())
                    .build()
                    .unwrap(),
            )
            .add_multi(multi1)
            .add_part(
                MimePart::builder()
                    .header("key4", "value4")
                    .body_from_reader("444444".as_bytes()) // nothing in async
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();

        let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
        let mut buf = vec![0u8; 30];
        encode_and_compare!(
            buf,
            {
                async_impl::Body::data(&mut multi_encoder, &mut buf)
                    .await
                    .unwrap()
            },
            vec![30, 30, 30, 30, 30, 30, 13],
            b"--abcde\r\nkey3:value3\r\n\r\n33333\r\n--abcde\r\n\
            content-type:multipart/mixed; boundary=abc\r\n\r\n--abc\r\n\
            key1:value1\r\n\r\n111111\r\n--abc\r\nkey2:value2\r\n\r\n22222\r\n\
            --abc--\r\n\r\n--abcde\r\nkey4:value4\r\n\r\n\r\n--abcde--\r\n"
        );
    }
}
