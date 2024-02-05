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
use std::collections::hash_map::IntoIter;

use crate::body::mime::common::{
    consume_crlf, data_copy, trim_front_lwsp, BytesResult, TokenResult,
};
use crate::body::mime::{CR, LF};
use crate::body::TokenStatus;
use crate::error::{ErrorKind, HttpError};
use crate::headers::{HeaderName, HeaderValue, Headers};
use crate::util::header_bytes::{HEADER_NAME_BYTES, HEADER_VALUE_BYTES};

#[derive(Debug, PartialEq)]
pub(crate) enum HeaderStatus {
    Start,
    Name,
    Colon,
    Value,
    Crlf,
    End,
}

#[derive(Debug)]
pub(crate) struct EncodeHeaders {
    pub(crate) stage: HeaderStatus,
    pub(crate) into_iter: IntoIter<HeaderName, HeaderValue>,
    pub(crate) value: Option<HeaderValue>,
    pub(crate) src: Vec<u8>,
    pub(crate) src_idx: usize,
}

impl EncodeHeaders {
    pub(crate) fn new(headers: Headers) -> Self {
        EncodeHeaders {
            stage: HeaderStatus::Start,
            into_iter: headers.into_iter(),
            value: None,
            src: vec![],
            src_idx: 0,
        }
    }

    // when the encode stage go to next
    fn check_next(&mut self) {
        match self.stage {
            HeaderStatus::Start => match self.into_iter.next() {
                Some((name, value)) => {
                    self.src = name.into_bytes();
                    self.src_idx = 0;
                    self.value = Some(value);
                    self.stage = HeaderStatus::Name;
                }
                None => {
                    self.stage = HeaderStatus::End;
                }
            },
            HeaderStatus::Name => {
                self.stage = HeaderStatus::Colon;
                self.src = b":".to_vec();
                self.src_idx = 0;
            }
            HeaderStatus::Colon => {
                self.stage = HeaderStatus::Value;
                match self.value.take() {
                    Some(v) => {
                        self.src = v.to_vec();
                    }
                    None => {
                        self.src = vec![];
                    }
                }
                self.src_idx = 0;
            }
            HeaderStatus::Value => {
                self.stage = HeaderStatus::Crlf;
                self.src = b"\r\n".to_vec();
                self.src_idx = 0;
            }
            HeaderStatus::Crlf => {
                self.stage = HeaderStatus::Start;
            }
            HeaderStatus::End => {}
        }
    }

    pub(crate) fn encode(&mut self, dst: &mut [u8]) -> TokenResult<usize> {
        match self.stage {
            HeaderStatus::Start => {
                self.check_next();
                Ok(TokenStatus::Partial(0))
            }
            HeaderStatus::Name | HeaderStatus::Colon | HeaderStatus::Value | HeaderStatus::Crlf => {
                match data_copy(&self.src, &mut self.src_idx, dst)? {
                    TokenStatus::Partial(size) => Ok(TokenStatus::Partial(size)),
                    TokenStatus::Complete(size) => {
                        self.check_next();
                        Ok(TokenStatus::Partial(size))
                    }
                }
            }
            HeaderStatus::End => Ok(TokenStatus::Complete(0)),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct DecodeHeaders {
    pub(crate) stage: HeaderStatus,
    pub(crate) name_src: Vec<u8>,
    pub(crate) src: Vec<u8>,
    pub(crate) headers: Headers,
}

impl DecodeHeaders {
    pub(crate) fn new() -> Self {
        DecodeHeaders {
            stage: HeaderStatus::Start,
            headers: Headers::new(),
            name_src: vec![],
            src: vec![],
        }
    }

    // when the decode stage go to next
    fn check_next(&mut self) {
        match self.stage {
            HeaderStatus::Start => {
                self.stage = HeaderStatus::Name;
            }
            HeaderStatus::Name => {
                self.stage = HeaderStatus::Value;
            }
            HeaderStatus::Colon => {}
            HeaderStatus::Value => {
                self.stage = HeaderStatus::Crlf;
            }
            HeaderStatus::Crlf => {
                self.stage = HeaderStatus::Start;
                self.src = vec![];
            }
            HeaderStatus::End => {}
        }
    }

    pub(crate) fn decode<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<(TokenStatus<Headers, ()>, &'a [u8]), HttpError> {
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput.into());
        }

        let mut results = TokenStatus::Partial(());
        let mut remains = buf;
        loop {
            let rest = match self.stage {
                HeaderStatus::Start => self.start_decode(remains),
                HeaderStatus::Name => self.name_decode(remains),
                // not use
                HeaderStatus::Colon => Ok(remains),
                HeaderStatus::Value => self.value_decode(remains),
                HeaderStatus::Crlf => self.crlf_decode(remains),
                HeaderStatus::End => {
                    results = TokenStatus::Complete(take(&mut self.headers));
                    break;
                }
            }?;
            remains = rest;
            if remains.is_empty() && self.stage != HeaderStatus::End {
                break;
            }
        }
        Ok((results, remains))
    }

    fn start_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let buf = if self.src.is_empty() {
            trim_front_lwsp(buf)
        } else {
            buf
        };

        let cr_meet = self.is_cr_meet();
        match buf[0] {
            CR => {
                if cr_meet {
                    Err(ErrorKind::InvalidInput.into())
                } else if buf.len() == 1 {
                    self.src.push(CR);
                    Ok(&[])
                } else if buf[1] == LF {
                    self.stage = HeaderStatus::End;
                    Ok(&buf[2..])
                } else {
                    Err(ErrorKind::InvalidInput.into())
                }
            }
            LF => {
                self.stage = HeaderStatus::End;
                Ok(&buf[1..])
            }
            _ => {
                if cr_meet {
                    Err(ErrorKind::InvalidInput.into())
                } else {
                    self.check_next();
                    Ok(buf)
                }
            }
        }
    }

    // check '\r'
    fn is_cr_meet(&self) -> bool {
        self.src.len() == 1 && self.src[0] == CR
    }

    fn crlf_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let cr_meet = self.is_cr_meet();
        match consume_crlf(buf, cr_meet)? {
            TokenStatus::Partial(_size) => Ok(&[]),
            TokenStatus::Complete(unparsed) => {
                self.check_next();
                Ok(unparsed)
            }
        }
    }

    fn name_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let buf = if self.src.is_empty() {
            trim_front_lwsp(buf)
        } else {
            buf
        };

        match Self::get_header_name(buf)? {
            TokenStatus::Partial(unparsed) => {
                self.src.extend_from_slice(unparsed);
                Ok(&[])
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.src.extend_from_slice(src);
                self.name_src = take(&mut self.src);
                self.check_next();
                Ok(unparsed)
            }
        }
    }

    fn value_decode<'a>(&mut self, buf: &'a [u8]) -> Result<&'a [u8], HttpError> {
        let buf = if self.src.is_empty() {
            trim_front_lwsp(buf)
        } else {
            buf
        };

        match Self::get_header_value(buf)? {
            TokenStatus::Partial(unparsed) => {
                self.src.extend_from_slice(unparsed);
                Ok(&[])
            }
            TokenStatus::Complete((src, unparsed)) => {
                // clone in this.
                self.src.extend_from_slice(src);
                let value = take(&mut self.src);
                let name = take(&mut self.name_src);

                self.headers
                    .insert(trim_front_lwsp(&name), trim_front_lwsp(&value))?;
                self.check_next();
                Ok(unparsed)
            }
        }
    }

    // end with ":"
    fn get_header_name(buf: &[u8]) -> BytesResult {
        for (i, b) in buf.iter().enumerate() {
            if *b == b':' {
                // match "k:v" or "k: v"
                return Ok(TokenStatus::Complete((&buf[..i], &buf[i + 1..])));
            } else if !HEADER_NAME_BYTES[*b as usize] {
                return Err(ErrorKind::InvalidInput.into());
            }
        }
        Ok(TokenStatus::Partial(buf))
    }

    // end with "\r" or "\n" or "\r\n"
    fn get_header_value(buf: &[u8]) -> BytesResult {
        for (i, b) in buf.iter().enumerate() {
            if *b == CR || *b == LF {
                return Ok(TokenStatus::Complete((&buf[..i], &buf[i..])));
            } else if !HEADER_VALUE_BYTES[*b as usize] {
                return Err(ErrorKind::InvalidInput.into());
            }
        }
        Ok(TokenStatus::Partial(buf))
    }
}

#[cfg(test)]
mod ut_decode_headers {
    use crate::body::mime::common::headers::DecodeHeaders;
    use crate::body::TokenStatus;
    use crate::headers::Headers;

    /// UT test cases for `DecodeHeaders::decode`.
    ///
    /// # Brief
    /// 1. Creates a `DecodeHeaders` by `DecodeHeaders::new`.
    /// 2. Uses `DecodeHeaders::decode` to decode headers.
    /// 3. The headers is divided by "\r\n".
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_new() {
        let buf = b"\r\nabcd";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(headers, TokenStatus::Complete(Headers::new()));
        assert_eq!(rest, b"abcd");

        // has LWSP
        let buf = b"    \r\nabcd";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(headers, TokenStatus::Complete(Headers::new()));
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `DecodeHeaders::decode`.
    ///
    /// # Brief
    /// 1. Creates a `DecodeHeaders` by `DecodeHeaders::new`.
    /// 2. Uses `DecodeHeaders::decode` to decode headers.
    /// 3. The headers is divided by "\n".
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_new2() {
        let buf = b"\nabcd";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(headers, TokenStatus::Complete(Headers::new()));
        assert_eq!(rest, b"abcd");

        // has LWSP
        let buf = b"    \nabcd";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(headers, TokenStatus::Complete(Headers::new()));
        assert_eq!(rest, b"abcd");
    }

    /// UT test cases for `DecodeHeaders::decode`.
    ///
    /// # Brief
    /// 1. Creates a `DecodeHeaders` by `DecodeHeaders::new`.
    /// 2. Uses `DecodeHeaders::decode` to decode headers.
    /// 3. The headers has *LWSP-char(b' ' or b'\t').
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_decode() {
        // all use "\r\n"
        let buf = b"    name1:   value1\r\n    name2:    value2\r\n\r\n";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(
            headers,
            TokenStatus::Complete({
                let mut headers = Headers::new();
                headers.insert("name1", "value1").unwrap();
                headers.insert("name2", "value2").unwrap();
                headers
            })
        );
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // all use "\n"
        let buf = b"name1:value1\nname2:value2\n\n";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(
            headers,
            TokenStatus::Complete({
                let mut headers = Headers::new();
                headers.insert("name1", "value1").unwrap();
                headers.insert("name2", "value2").unwrap();
                headers
            })
        );
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // some use "\r\n"
        let buf = b"name1:value1\nname2:value2\r\n\n";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(
            headers,
            TokenStatus::Complete({
                let mut headers = Headers::new();
                headers.insert("name1", "value1").unwrap();
                headers.insert("name2", "value2").unwrap();
                headers
            })
        );
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");
    }

    /// UT test cases for `DecodeHeaders::decode`.
    ///
    /// # Brief
    /// 1. Creates a `DecodeHeaders` by `DecodeHeaders::new`.
    /// 2. Uses `DecodeHeaders::decode` to decode headers.
    /// 3. The headers is common.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_decode2() {
        let buf = b"name1:value1\r\n\r\n\r\naaaa";
        let mut decoder = DecodeHeaders::new();
        let (headers, rest) = decoder.decode(buf).unwrap();
        assert_eq!(
            headers,
            TokenStatus::Complete({
                let mut headers = Headers::new();
                headers.insert("name1", "value1").unwrap();
                headers
            })
        );
        assert_eq!(std::str::from_utf8(rest).unwrap(), "\r\naaaa");
    }

    /// UT test cases for `DecodeHeaders::decode`.
    ///
    /// # Brief
    /// 1. Creates a `DecodeHeaders` by `DecodeHeaders::new`.
    /// 2. Uses `DecodeHeaders::decode` to decode headers.
    /// 3. The decode bytes are divided into several executions.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_decode_headers_decode3() {
        let buf = b"name1:value1\r\nname2:value2\r\n\r\naaaa";
        let mut decoder = DecodeHeaders::new();
        // nam
        let (headers, rest) = decoder.decode(&buf[0..3]).unwrap();
        assert_eq!(headers, TokenStatus::Partial(()));
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // e1:value1\r
        let (headers, rest) = decoder.decode(&buf[3..13]).unwrap();
        assert_eq!(headers, TokenStatus::Partial(()));
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // \nname2:value2\r\n\r
        let (headers, rest) = decoder.decode(&buf[13..29]).unwrap();
        assert_eq!(headers, TokenStatus::Partial(()));
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // \n
        let (headers, rest) = decoder.decode(&buf[29..30]).unwrap();
        assert_eq!(
            headers,
            TokenStatus::Complete({
                let mut headers = Headers::new();
                headers.insert("name1", "value1").unwrap();
                headers.insert("name2", "value2").unwrap();
                headers
            })
        );
        assert_eq!(std::str::from_utf8(rest).unwrap(), "");

        // aaaa
        let (headers, rest) = decoder.decode(&buf[30..34]).unwrap();
        assert_eq!(headers, TokenStatus::Complete(Headers::new()));
        assert_eq!(std::str::from_utf8(rest).unwrap(), "aaaa");
    }
}
