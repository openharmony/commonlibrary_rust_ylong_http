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

use core::mem::take;

use crate::body::mime::common::{
    get_content_type_boundary, get_crlf_contain, trim_back_lwsp_if_end_with_lf, XPart,
};
use crate::body::mime::decode::BoundaryTag;
use crate::body::mime::{DecodeHeaders, MimeMulti, MimePartDecoder};
use crate::body::TokenStatus;
use crate::error::{ErrorKind, HttpError};
use crate::headers::Headers;

type ByteVec<'a> = Result<TokenStatus<(Vec<u8>, &'a [u8]), &'a [u8]>, HttpError>;

// TODO: Increases compatibility for preamble and epilogue.

/// `MimeMultiDecoder` can create a [`MimeMulti`] according to a serialized
/// data.
///
/// [`MimeMulti`]: MimeMulti
///
/// # Examples
///
/// ```
/// use ylong_http::body::{MimeMulti, MimeMultiDecoder, MimePart, TokenStatus};
///
/// let buf = b"--abc\r\nkey1:value1\r\n\r\n1111\r\n--abc--\r\nabcd";
/// let mut decoder = MimeMultiDecoder::new();
/// let (elem, rest) = decoder.decode(buf).unwrap();
/// assert!(elem.is_complete());
/// let multi1 = MimeMulti::builder()
///     .set_boundary(b"abc".to_vec())
///     .add_part(
///         MimePart::builder()
///             .header("key1", "value1")
///             .body_from_owned(b"1111".to_vec())
///             .build()
///             .unwrap(),
///     )
///     .build()
///     .unwrap();
/// if let TokenStatus::Complete(multi) = elem {
///     assert_eq!(multi.boundary(), multi1.boundary());
///     assert_eq!(multi.headers(), multi1.headers());
///     assert_eq!(multi, multi1);
/// }
/// assert_eq!(rest, b"abcd");
/// ```
#[derive(Debug, Default, PartialEq)]
pub struct MimeMultiDecoder {
    // stack of stage
    stages: Vec<MultiStage>,
    // stack of multi
    multis: Vec<MimeMulti<'static>>,
}

impl MimeMultiDecoder {
    /// Creates a new `MimeMultiDecoder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiDecoder;
    ///
    /// let mut decoder = MimeMultiDecoder::new();
    /// ```
    pub fn new() -> Self {
        let mut decoder = MimeMultiDecoder {
            stages: vec![],
            multis: vec![],
        };
        let multi = MimeMulti::new();
        decoder.multis.push(multi);
        let data = DecodeData::new();
        decoder.stages.push(MultiStage::Multi(data));
        decoder
    }

    /// Inputs datas, gets a [`MimeMulti`].
    ///
    /// [`MimeMulti`]: MimeMulti
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimeMulti, MimeMultiDecoder, MimePart, TokenStatus};
    ///
    /// let buf = b"--abc\r\n\r\n--abc--\r\nabcd";
    /// let mut decoder = MimeMultiDecoder::new();
    /// let (elem, rest) = decoder.decode(buf).unwrap();
    /// assert!(elem.is_complete());
    /// let multi1 = MimeMulti::builder()
    ///     .set_boundary(b"abc".to_vec())
    ///     .add_part(MimePart::builder().build().unwrap())
    ///     .build()
    ///     .unwrap();
    /// if let TokenStatus::Complete(multi) = elem {
    ///     assert_eq!(multi.boundary(), multi1.boundary());
    ///     assert_eq!(multi, multi1);
    /// }
    /// assert_eq!(rest, b"abcd");
    /// ```
    pub fn decode<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<(TokenStatus<MimeMulti, ()>, &'a [u8]), HttpError> {
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput.into());
        }

        let mut results = TokenStatus::Partial(());
        let mut remains = buf;

        while let Some(stage) = self.stages.pop() {
            let rest = match stage {
                MultiStage::Unknown(headers_decoder) => {
                    self.unknown_decode(headers_decoder, remains)
                }
                MultiStage::Part(part_decoder) => self.part_decode(part_decoder, remains),
                MultiStage::Multi(data) => self.multi_decode(data, remains),
                MultiStage::End => {
                    results = match self.multis.pop() {
                        Some(multi) => TokenStatus::Complete(multi),
                        None => return Err(ErrorKind::InvalidInput.into()),
                    };
                    break;
                }
            }?;
            remains = rest;
            // at least has the outermost multi stage, unless is replaced by
            // MultiStage::End, so the last stage can uncheck.
            if remains.is_empty() && !self.last_stage()?.is_end() {
                break;
            }
        }

        Ok((results, remains))
    }

    fn unknown_decode<'a>(
        &mut self,
        mut headers_decoder: DecodeHeaders,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        let (elem, rest) = headers_decoder.decode(buf)?;
        match elem {
            TokenStatus::Partial(_) => self.stages.push(MultiStage::Unknown(headers_decoder)),
            TokenStatus::Complete(headers) => {
                match get_content_type_boundary(&headers) {
                    // is multi
                    Some(boundary) => self.push_new_multi_stage(headers, boundary),
                    // is part
                    None => self.push_new_part_stage(headers)?,
                }
            }
        }
        Ok(rest)
    }

    fn push_new_unknown_stage(&mut self) {
        let headers_decoder = DecodeHeaders::new();
        self.stages.push(MultiStage::Unknown(headers_decoder));
    }

    fn push_new_multi_stage(&mut self, headers: Headers, boundary: Vec<u8>) {
        // push a new Multi stage
        let data = DecodeData::new_as_part(boundary.clone());
        self.stages.push(MultiStage::Multi(data));

        // push a new multi struct
        let mut multi = MimeMulti::new();
        multi.set_headers(headers);
        multi.set_boundary(boundary);
        self.multis.push(multi);
    }

    fn push_new_part_stage(&mut self, headers: Headers) -> Result<(), HttpError> {
        let mut decoder = MimePartDecoder::new();
        // at least has the outermost multi, so the last multi can uncheck.
        decoder.init_from_multi(headers, self.last_multi()?.boundary.clone());
        self.stages.push(MultiStage::Part(decoder));
        Ok(())
    }

    fn part_decode<'a>(
        &mut self,
        mut part_decoder: MimePartDecoder,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        let (elem, rest) = part_decoder.decode(buf)?;
        match elem {
            TokenStatus::Partial(_) => {
                // push self
                self.stages.push(MultiStage::Part(part_decoder));
            }
            TokenStatus::Complete(part) => {
                // at least has the outermost multi, so the last multi can uncheck.
                self.last_multi_mut()?.add_part(part);
                // now temp multi is end
                if part_decoder.is_last_part() {
                    self.temp_multi_end(true)?;
                } else {
                    self.push_new_unknown_stage();
                }
            }
        }
        Ok(rest)
    }

    fn multi_decode<'a>(&mut self, data: DecodeData, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        match data.is_finish_first_boundary {
            true => self.middle_or_end_boundary_decode(data, buf),
            false => self.first_boundary_decode(data, buf),
        }
    }

    /// find middle or end boundary for inner and outer multi
    fn middle_or_end_boundary_decode<'a>(
        &mut self,
        mut data: DecodeData,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        match data.middle_or_end_boundary(buf) {
            TokenStatus::Partial(rest) => {
                self.stages.push(MultiStage::Multi(data));
                Ok(rest)
            }
            TokenStatus::Complete(rest) => match data.tag {
                BoundaryTag::Middle => {
                    self.stages.push(MultiStage::Multi(data));
                    self.push_new_unknown_stage();
                    Ok(rest)
                }
                BoundaryTag::End => {
                    self.temp_multi_end(false)?;
                    Ok(rest)
                }
                // ensure not in this arm
                _ => Ok(rest),
            },
        }
    }

    fn first_boundary_decode<'a>(
        &mut self,
        data: DecodeData,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        match data.is_outermost {
            true => self.outermost_first_boundary_decode(data, buf),
            false => self.inner_first_boundary_decode(data, buf),
        }
    }

    /// for outermost multi
    fn outermost_first_boundary_decode<'a>(
        &mut self,
        mut data: DecodeData,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        match data.outermost_first_boundary(buf)? {
            TokenStatus::Partial(rest) => {
                // push self
                self.stages.push(MultiStage::Multi(data));
                Ok(rest)
            }
            TokenStatus::Complete((boundary, rest)) => {
                match self.multis.first_mut() {
                    Some(multi) => {
                        multi.set_boundary(boundary);
                        // push self
                        self.stages.push(MultiStage::Multi(data));
                        self.push_new_unknown_stage();
                        Ok(rest)
                    }
                    None => Err(ErrorKind::InvalidInput.into()),
                }
            }
        }
    }

    /// for inner multi
    fn inner_first_boundary_decode<'a>(
        &mut self,
        mut data: DecodeData,
        buf: &'a [u8],
    ) -> Result<&'a [u8], HttpError> {
        let rest = match data.inner_first_boundary(buf)? {
            TokenStatus::Partial(rest) => rest,
            TokenStatus::Complete(rest) => rest,
        };
        // push self
        self.stages.push(MultiStage::Multi(data));
        self.push_new_unknown_stage();
        Ok(rest)
    }

    // when the last multi is end.
    fn temp_multi_end(&mut self, by_part: bool) -> Result<(), HttpError> {
        if self.multis.len() == 1 {
            self.stages = vec![MultiStage::End];
            return Ok(());
        }

        if by_part {
            // pop the nearly Multi stage
            self.stages_pop()?;
        }
        let multi = self.multis_pop()?;
        // at least has the outermost multi, so the last multi can uncheck.
        self.last_multi_mut()?.add_multi(multi);
        Ok(())
    }

    fn multis_pop(&mut self) -> Result<MimeMulti<'static>, HttpError> {
        match self.multis.pop() {
            Some(multi) => Ok(multi),
            None => Err(ErrorKind::InvalidInput.into()),
        }
    }

    fn stages_pop(&mut self) -> Result<MultiStage, HttpError> {
        match self.stages.pop() {
            Some(stage) => Ok(stage),
            None => Err(ErrorKind::InvalidInput.into()),
        }
    }

    fn last_multi(&self) -> Result<&MimeMulti<'static>, HttpError> {
        match self.multis.last() {
            Some(multi) => Ok(multi),
            None => Err(ErrorKind::InvalidInput.into()),
        }
    }

    fn last_multi_mut(&mut self) -> Result<&mut MimeMulti<'static>, HttpError> {
        match self.multis.last_mut() {
            Some(multi) => Ok(multi),
            None => Err(ErrorKind::InvalidInput.into()),
        }
    }

    fn last_stage(&self) -> Result<&MultiStage, HttpError> {
        match self.stages.last() {
            Some(stage) => Ok(stage),
            None => Err(ErrorKind::InvalidInput.into()),
        }
    }
}

#[derive(Debug, PartialEq)]
enum MultiStage {
    Unknown(DecodeHeaders),
    Part(MimePartDecoder),
    Multi(DecodeData),
    End,
}

impl MultiStage {
    fn is_end(&self) -> bool {
        matches!(self, MultiStage::End)
    }
}

#[derive(Debug, PartialEq)]
struct DecodeData {
    // whether read first boundary completely
    is_finish_first_boundary: bool,
    // whether is outermost multi
    is_outermost: bool,
    boundary: Vec<u8>,
    // 1 is middle part; 2 is end part
    tag: BoundaryTag,
    // src which is need to encode
    src: Vec<u8>,
    src_idx: usize,
}

impl DecodeData {
    /// is outermost multi
    fn new() -> Self {
        Self {
            is_finish_first_boundary: false,
            is_outermost: true,
            boundary: vec![],
            tag: BoundaryTag::Init,
            src: vec![],
            src_idx: 0,
        }
    }

    /// is a part of multi
    fn new_as_part(boundary: Vec<u8>) -> Self {
        Self {
            is_finish_first_boundary: false,
            is_outermost: false,
            boundary,
            tag: BoundaryTag::Init,
            src: vec![],
            src_idx: 0,
        }
    }

    fn set_boundary(&mut self, boundary: Vec<u8>) {
        self.boundary = boundary;
    }

    /// finds the first boundary of the outermost multi
    fn outermost_first_boundary<'a>(&mut self, buf: &'a [u8]) -> ByteVec<'a> {
        match get_crlf_contain(buf) {
            TokenStatus::Partial(unparsed) => {
                self.src.extend_from_slice(unparsed);
                Ok(TokenStatus::Partial(&[]))
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.src.extend_from_slice(src);
                // safety: use for trim.
                let s = unsafe { std::str::from_utf8_unchecked(&self.src).trim() };
                // implies s >= 2
                if let Some(s) = s.strip_prefix("--") {
                    let boundary = s.as_bytes().to_vec();
                    self.set_boundary(boundary.clone());
                    self.when_is_first_boundary();
                    return Ok(TokenStatus::Complete((boundary, unparsed)));
                }

                // is not end; need to find next '\n'
                self.src = vec![];
                self.src_idx = 0;
                Ok(TokenStatus::Partial(&[]))
            }
        }
    }

    /// use to inner multi
    fn inner_first_boundary<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<TokenStatus<&'a [u8], &'a [u8]>, HttpError> {
        match get_crlf_contain(buf) {
            TokenStatus::Partial(unparsed) => {
                self.src.extend_from_slice(unparsed);
                Ok(TokenStatus::Partial(&[]))
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.src.extend_from_slice(src);
                let line = &self.src[self.src_idx..];
                let trim_line = trim_back_lwsp_if_end_with_lf(line);

                // checks whether is first boundary
                let mut deriv_boundary = b"--".to_vec();
                deriv_boundary.extend_from_slice(&self.boundary);
                if trim_line == deriv_boundary {
                    self.when_is_first_boundary();
                    return Ok(TokenStatus::Complete(unparsed));
                }

                // is not end
                self.src_idx = self.src.len();
                Ok(TokenStatus::Partial(unparsed))
            }
        }
    }

    fn when_is_first_boundary(&mut self) {
        self.is_finish_first_boundary = true;
        self.tag = BoundaryTag::First;
        self.src_init();
    }

    fn src_init(&mut self) {
        // init
        self.src = vec![];
        self.src_idx = 0;
    }

    /// use to all multi
    fn middle_or_end_boundary<'a>(&mut self, buf: &'a [u8]) -> TokenStatus<&'a [u8], &'a [u8]> {
        match get_crlf_contain(buf) {
            TokenStatus::Partial(unparsed) => {
                self.src.extend_from_slice(unparsed);
                TokenStatus::Partial(&[])
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.src.extend_from_slice(src);
                let line = &self.src[self.src_idx..];
                let trim_line = trim_back_lwsp_if_end_with_lf(line);

                // checks whether is middle boundary
                let mut deriv_boundary = b"--".to_vec();
                deriv_boundary.extend_from_slice(&self.boundary);
                if trim_line == deriv_boundary {
                    self.tag = BoundaryTag::Middle;
                    self.src_init();
                    return TokenStatus::Complete(unparsed);
                }
                // checks whether is end boundary
                deriv_boundary.extend_from_slice(b"--");
                if trim_line == deriv_boundary {
                    self.tag = BoundaryTag::End;
                    self.src_init();
                    return TokenStatus::Complete(unparsed);
                }

                // is not end
                self.src_idx = self.src.len();
                TokenStatus::Partial(unparsed)
            }
        }
    }
}

#[cfg(test)]
mod ut_mime_multi_decoder {
    use crate::body::{MimeMulti, MimeMultiDecoder, MimePart, TokenStatus};

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of a part.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_new() {
        let buf = b"--abc\r\n\r\n--abc--\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(MimePart::builder().build().unwrap())
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi.boundary(), multi1.boundary());
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of a part.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_one_part() {
        let buf = b"--abc\r\nkey1:value1\r\n\r\n1111\r\n--abc--\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_owned(b"1111".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi.boundary(), multi1.boundary());
            assert_eq!(multi.headers(), multi1.headers());
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of a part.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_one_part_no_headers() {
        let buf = b"--abc\r\n\r\n112233\r\n--abc--\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .body_from_owned(b"112233".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi.boundary(), multi1.boundary());
            assert_eq!(multi.headers(), multi1.headers());
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of a part.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_one_part_no_body() {
        let buf = b"--abc\r\nkey1:value1\r\n\r\n--abc--\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi.boundary(), multi1.boundary());
            assert_eq!(multi.headers(), multi1.headers());
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of several parts.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_several_parts() {
        let buf =
            b"---\r\nkey1:value1\r\n\r\n1111\r\n---\r\nkey2: value2\r\n\r\n2222\r\n-----\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"-".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_owned(b"1111".to_vec())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is composed of several parts, and the boundary has
    ///    LWSP chars.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_several_parts_has_lwsp() {
        let buf =
            b"---    \r\nkey1:value1\r\n\r\n1111\r\n---   \r\nkey2: value2\r\n\r\n2222\r\n-----     \r\nabcd";
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"-".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_owned(b"1111".to_vec())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is nesting.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_nest() {
        let buf = "\
        --abc\r\nContent-Type:multipart/mix; boundary=aaa\r\n\r\n\
        --aaa\r\nkey1:value1\r\n\r\n1111\r\n--aaa--\r\n\r\n--abc--\r\nabcd"
            .as_bytes();
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_multi(
                MimeMulti::builder()
                    .set_content_type(b"multipart/mix", b"aaa".to_vec())
                    .add_part(
                        MimePart::builder()
                            .header("key1", "value1")
                            .body_from_owned(b"1111".to_vec())
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is nesting.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_nest2() {
        let buf = "\
        --abc\r\nContent-Type:multipart/mix; boundary=aaa\r\n\r\n--aaa\r\nkey1:\
        value1\r\n\r\n1111\r\n--aaa--\r\n\r\n--abc\r\nkey2:value2\r\n\r\n2222\r\n--\
        abc--\r\nabcd"
            .as_bytes();
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_multi(
                MimeMulti::builder()
                    .set_content_type(b"multipart/mix", b"aaa".to_vec())
                    .add_part(
                        MimePart::builder()
                            .header("key1", "value1")
                            .body_from_owned(b"1111".to_vec())
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is nesting.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_nest3() {
        let buf = "\
        --abc\r\nkey2:value2\r\n\r\n2222\r\n--abc\r\nContent-Type:multipart/mix; \
        boundary=aaa\r\n\r\n--aaa\r\nkey1:value1\r\n\r\n1111\r\n--aaa--\r\n\r\n\
        --abc--\r\nabcd"
            .as_bytes();
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .add_multi(
                MimeMulti::builder()
                    .set_content_type(b"multipart/mix", b"aaa".to_vec())
                    .add_part(
                        MimePart::builder()
                            .header("key1", "value1")
                            .body_from_owned(b"1111".to_vec())
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The `MimeMulti` is nesting.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_nest4() {
        let buf = "\
        --abc\r\nkey2:value2\r\n\r\n2222\r\n--abc\r\nContent-Type:multipart/mix; \
        boundary=aaa\r\n\r\n--aaa\r\nkey1:value1\r\n\r\n1111\r\n--aaa\r\nkey3:value3\
        \r\n\r\n--aaa--\r\n\r\n--abc--\r\nabcd"
            .as_bytes();
        let mut decoder = MimeMultiDecoder::new();
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"abc".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .add_multi(
                MimeMulti::builder()
                    .set_content_type(b"multipart/mix", b"aaa".to_vec())
                    .add_part(
                        MimePart::builder()
                            .header("key1", "value1")
                            .body_from_owned(b"1111".to_vec())
                            .build()
                            .unwrap(),
                    )
                    .add_part(
                        MimePart::builder()
                            .header("key3", "value3")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. The decode bytes are divided into several executions.
    /// 4. Creates a `MimeMulti` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_times() {
        let buf =
            b"---\r\nkey1:value1\r\n\r\n1111\r\n---\r\nkey2: value2\r\n\r\n2222\r\n-----\r\nabcd";
        let mut decoder = MimeMultiDecoder::new();

        //---
        let (elem, rest) = decoder.decode(&buf[..3]).unwrap();
        assert!(!elem.is_complete());
        assert_eq!(rest, b"");

        //\r\nkey1:value1
        let (elem, rest) = decoder.decode(&buf[3..16]).unwrap();
        assert!(!elem.is_complete());
        assert_eq!(rest, b"");

        //\r\n\r\n1111\r\n---\r\n
        let (elem, rest) = decoder.decode(&buf[16..31]).unwrap();
        assert!(!elem.is_complete());
        assert_eq!(rest, b"");

        // key2: value2\r\n\r\n2222\r\n-----\r\n
        let (elem, rest) = decoder.decode(&buf[31..60]).unwrap();
        assert!(elem.is_complete());
        let multi1 = MimeMulti::builder()
            .set_boundary(b"-".to_vec())
            .add_part(
                MimePart::builder()
                    .header("key1", "value1")
                    .body_from_owned(b"1111".to_vec())
                    .build()
                    .unwrap(),
            )
            .add_part(
                MimePart::builder()
                    .header("key2", "value2")
                    .body_from_owned(b"2222".to_vec())
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        if let TokenStatus::Complete(multi) = elem {
            assert_eq!(multi, multi1);
        }
        assert_eq!(rest, b"");

        // abcd
        let (_elem, rest) = decoder.decode(&buf[60..]).unwrap();
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimeMultiDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimeMultiDecoder` by `MimeMultiDecoder::new`.
    /// 2. Uses `MimeMultiDecoder::decode` to decode `MimeMulti`.
    /// 3. Uses wrong input.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_multi_decoder_err() {
        let mut buf = b"--abc\r\n\r\n--abc--\r\nabcd".to_vec();
        buf.insert(7, 200);
        let mut decoder = MimeMultiDecoder::new();
        let res = decoder.decode(&buf);
        assert!(res.is_err());
    }
}
