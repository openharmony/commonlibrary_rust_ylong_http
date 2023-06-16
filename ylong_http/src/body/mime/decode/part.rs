/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::{
    body::{
        mime::{
            common::{get_crlf_contain, trim_back_lwsp_if_end_with_lf, trim_front_lwsp},
            decode::BoundaryTag,
            DecodeHeaders, MimePart, PartStatus,
        },
        TokenStatus,
    },
    error::{ErrorKind, HttpError},
    headers::Headers,
};
use core::mem::take;

#[derive(Debug, PartialEq)]
pub(crate) struct MimePartDecoder {
    stage: PartStatus, // Encode stage now
    headers_decoder: DecodeHeaders,
    part: MimePart<'static>,
    boundary: Vec<u8>, // boundary
    tag: BoundaryTag,
    src_idx: usize, // marks last "\n"
}

impl MimePartDecoder {
    pub(crate) fn new() -> Self {
        MimePartDecoder {
            stage: PartStatus::Start,
            headers_decoder: DecodeHeaders::new(),
            part: MimePart::new(),
            boundary: b"-".to_vec(),
            tag: BoundaryTag::Init,
            src_idx: 0,
        }
    }

    pub(crate) fn init(&mut self) {
        self.stage = PartStatus::Start;
        self.headers_decoder = DecodeHeaders::new();

        self.part = MimePart::new();
        self.boundary = b"-".to_vec();
        self.tag = BoundaryTag::Init;
        self.src_idx = 0;
    }

    // by MimeMultiDecode
    pub(crate) fn init_from_multi(&mut self, headers: Headers, boundary: Vec<u8>) {
        self.init();
        self.part.set_headers(headers);
        self.boundary = boundary;
        self.stage = PartStatus::Body;
    }

    pub(crate) fn set_boundary(&mut self, boundary: Vec<u8>) {
        self.boundary = boundary;
    }

    pub(crate) fn decode<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<(TokenStatus<MimePart<'static>, ()>, &'a [u8]), HttpError> {
        // Option
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput.into());
        }

        let mut results = TokenStatus::Partial(());
        let mut remains = buf;
        loop {
            let rest = match self.stage {
                PartStatus::Start => self.start_decode(remains),
                PartStatus::Headers => self.headers_decode(remains),
                PartStatus::Crlf => Ok(remains), // not use
                PartStatus::Body => self.body_decode(remains),
                PartStatus::End => {
                    results = TokenStatus::Complete(take(&mut self.part));
                    break;
                }
            }?;
            remains = rest;
            if remains.is_empty() && self.stage != PartStatus::End {
                break;
            }
        }
        Ok((results, remains))
    }

    fn start_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let buf = trim_front_lwsp(buf);
        self.stage = PartStatus::Headers;
        Ok(buf)
    }

    // Headers + Crlf
    fn headers_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let (elem, rest) = self.headers_decoder.decode(buf)?;
        if let TokenStatus::Complete(headers) = elem {
            self.part.set_headers(headers);
            self.stage = PartStatus::Body;
        }

        Ok(rest)
    }

    // Writes to the part body and checks boundary.
    fn body_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        match get_crlf_contain(buf) {
            TokenStatus::Partial(unparsed) => {
                self.part.body_extend_from_slice(unparsed);
                Ok(&[])
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.part.body_extend_from_slice(src);
                let body = match self.part.body() {
                    Some(vec) => vec,
                    None => return Err(ErrorKind::InvalidInput.into()),
                };
                let line = &body[self.src_idx..];
                let trim_line = trim_back_lwsp_if_end_with_lf(line);

                // checks whether is middle boundary
                let mut deriv_boundary = b"--".to_vec();
                deriv_boundary.extend_from_slice(&self.boundary);
                if trim_line == deriv_boundary {
                    self.tag = BoundaryTag::Middle;
                    self.when_is_end();
                    return Ok(unparsed);
                }
                // checks whether is end boundary
                deriv_boundary.extend_from_slice(b"--");
                if trim_line == deriv_boundary {
                    self.tag = BoundaryTag::End;
                    self.when_is_end();
                    return Ok(unparsed);
                }

                // is not end
                self.src_idx = body.len();
                Ok(unparsed)
            }
        }
    }

    fn when_is_end(&mut self) {
        self.stage = PartStatus::End;
        self.part.body_truncate(self.src_idx);
        self.part.body_trim_crlf_once();
    }

    // Checks whether is the last part of multi.
    pub(crate) fn is_last_part(&self) -> bool {
        self.tag == BoundaryTag::End
    }
}

#[cfg(test)]
mod ut_mime_part_decoder {
    use crate::body::{
        mime::{MimePart, MimePartDecoder},
        TokenStatus,
    };

    /// UT test cases for `MimePartDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartDecoder` by `MimePartDecoder::new`.
    /// 2. Uses `MimePartDecoder::decode` to decode `MimePart`.
    /// 3. Creates a `MimePart` and sets the same parameters to compare.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_decoder_new_by_crlf() {
        let part1 = MimePart::builder()
            .body_from_owned(b"abcd".to_vec())
            .build()
            .unwrap();

        // common
        let buf = b"\r\nabcd\r\n--abc\r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");

        // has LWSP
        let buf = b"   \r\nabcd\r\n--abc\r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");

        // has RFC LWSP
        let buf = b"\r\nabcd\r\n--abc    \r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimePartDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartDecoder` by `MimePartDecoder::new`.
    /// 2. Uses `MimePartDecoder::decode` to decode `MimePart`.
    /// 3. Creates a `MimePart` and sets the same parameters to compare.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_decoder_new_by_lf() {
        let part1 = MimePart::builder()
            .body_from_owned(b"abcd".to_vec())
            .build()
            .unwrap();

        // common
        let buf = b"\nabcd\n--abc\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");

        // has LWSP
        let buf = b"    \nabcd\n--abc\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");

        // has RFC LWSP
        let buf = b"    \nabcd\n--abc   \nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());

        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimePartDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartDecoder` by `MimePartDecoder::new`.
    /// 2. Uses `MimePartDecoder::decode` to decode `MimePart`.
    /// 3. Creates a `MimePart` and sets the same parameters to compare.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_decoder_decode() {
        let buf = b"    name1:   value1\r\n    name2:    value2\r\n\r\nabcd\r\n--abc  \r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let part1 = MimePart::builder()
            .header("name1", "value1")
            .header("name2", "value2")
            .body_from_owned(b"abcd".to_vec())
            .build()
            .unwrap();
        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimePartDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartDecoder` by `MimePartDecoder::new`.
    /// 2. Uses `MimePartDecoder::decode` to decode `MimePart`.
    /// 3. Creates a `MimePart` and sets the same parameters to compare.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_decoder_no_headers_no_body() {
        let buf = b"\r\n--abc\r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(buf).unwrap();
        assert!(elem.is_complete());
        let part1 = MimePart::builder().build().unwrap();
        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `MimePartDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartDecoder` by `MimePartDecoder::new`.
    /// 2. Uses `MimePartDecoder::decode` to decode `MimePart`.
    /// 3. The decode bytes are divided into several executions.
    /// 4. Creates a `MimePart` and sets the same parameters to compare.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_decode_times() {
        let buf = b"name1:value1\r\nname2:value2\r\n\r\nabcd\r\n--abc\r\nabcd";
        let mut decoder = MimePartDecoder::new();
        decoder.set_boundary(b"abc".to_vec());
        let (elem, rest) = decoder.decode(&buf[0..3]).unwrap(); // nam
        assert!(!elem.is_complete());
        assert_eq!(rest, b"");

        let (elem, rest) = decoder.decode(&buf[3..30]).unwrap(); // e1:value1\r\nname2:value2\r\n\r\n
        assert!(!elem.is_complete());
        assert_eq!(rest, b"");

        let (elem, rest) = decoder.decode(&buf[30..]).unwrap(); // abcd\r\n--abc\r\nabcd
        assert!(elem.is_complete());
        let part1 = MimePart::builder()
            .header("name1", "value1")
            .header("name2", "value2")
            .body_from_owned(b"abcd".to_vec())
            .build()
            .unwrap();
        if let TokenStatus::Complete(part) = elem {
            assert_eq!(part, part1);
        }
        assert_eq!(rest, b"abcd");
    }
}
