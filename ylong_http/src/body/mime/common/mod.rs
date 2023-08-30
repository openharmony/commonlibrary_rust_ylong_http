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

mod headers;
mod mix;
mod multi;
mod part;

pub(crate) use headers::{DecodeHeaders, EncodeHeaders, HeaderStatus};
pub(crate) use mix::MixFrom;
pub use multi::{MimeMulti, MimeMultiBuilder, XPart};
pub(crate) use part::PartStatus;
pub use part::{MimePart, MimePartBuilder};
pub(crate) type SizeResult = Result<usize, std::io::Error>;
pub(crate) type TokenResult<T> = Result<TokenStatus<usize, T>, std::io::Error>;
pub(crate) type BytesResult<'a> = Result<TokenStatus<(&'a [u8], &'a [u8]), &'a [u8]>, HttpError>;

use core::mem::take;
use std::io::Read;

use crate::error::{ErrorKind, HttpError};
use crate::headers::Headers;

// RFC5234 ABNF
// horizontal tab
pub(crate) const HTAB: u8 = b'\t';
// 0x20 space
pub(crate) const SP: u8 = b' ';
// carriage return
pub(crate) const CR: u8 = b'\r';
// linefeed
pub(crate) const LF: u8 = b'\n';
pub(crate) const CRLF: &[u8] = b"\r\n";

/// Represents component encoding/decoding status.
#[derive(Debug, Eq, PartialEq)]
pub enum TokenStatus<T, E> {
    /// The current component is partially encoded.
    Partial(E),
    /// The current component is completely encoded.
    Complete(T),
}

impl<T, E> TokenStatus<T, E> {
    /// Checks whether is TokenStatus::Complete(T).
    pub fn is_complete(&self) -> bool {
        match self {
            TokenStatus::Partial(_) => false,
            TokenStatus::Complete(_) => true,
        }
    }

    /// Gets the complete inner type.
    pub fn get_complete_once(self) -> Option<T> {
        match self {
            TokenStatus::Partial(_) => None,
            TokenStatus::Complete(inner) => Some(inner),
        }
    }
}

// Pulls some bytes from this src into the buf, returning how many bytes were
// read.
pub(crate) fn data_copy(src: &[u8], src_idx: &mut usize, buf: &mut [u8]) -> TokenResult<usize> {
    let input_len = src.len() - *src_idx;
    let output_len = buf.len();
    // sync
    let num = (&src[*src_idx..]).read(buf)?;
    *src_idx += num;
    if output_len >= input_len {
        return Ok(TokenStatus::Complete(num));
    }
    Ok(TokenStatus::Partial(num))
}

// removes front *LWSP-char(b' ' or b'\t')
pub(crate) fn trim_front_lwsp(buf: &[u8]) -> &[u8] {
    let mut idx = 0;
    for b in buf.iter() {
        match *b {
            SP | HTAB => idx += 1,
            _ => break,
        }
    }
    &buf[idx..]
}

// removes back *LWSP-char(b' ' or b'\t')
fn trim_back_lwsp(buf: &[u8]) -> &[u8] {
    let mut idx = 0;
    for b in buf.iter().rev() {
        match *b {
            SP | HTAB => idx += 1,
            _ => break,
        }
    }
    &buf[..buf.len() - idx]
}

/// buf is not empty, and end with '\n'.
/// removes back *LWSP-char(b' ' or b'\t')
pub(crate) fn trim_back_lwsp_if_end_with_lf(buf: &[u8]) -> &[u8] {
    let mut temp = &buf[..buf.len() - 1];
    if temp.ends_with(&[CR]) {
        temp = &temp[..temp.len() - 1];
    }
    trim_back_lwsp(temp)
}

// reduce "\n" or "\r\n"
pub(crate) fn consume_crlf(
    buf: &[u8],
    // has "\r"
    cr_meet: bool,
) -> Result<TokenStatus<&[u8], usize>, HttpError> {
    if buf.is_empty() {
        return Ok(TokenStatus::Partial(0));
    }
    match buf[0] {
        CR => {
            if cr_meet {
                Err(ErrorKind::InvalidInput.into())
            } else if buf.len() == 1 {
                Ok(TokenStatus::Partial(1))
            } else if buf[1] == LF {
                Ok(TokenStatus::Complete(&buf[2..]))
            } else {
                Err(ErrorKind::InvalidInput.into())
            }
        }
        LF => Ok(TokenStatus::Complete(&buf[1..])),
        _ => Err(ErrorKind::InvalidInput.into()),
    }
}

// end with "\n" or "\r\n"
pub(crate) fn get_crlf_contain(buf: &[u8]) -> TokenStatus<(&[u8], &[u8]), &[u8]> {
    for (i, b) in buf.iter().enumerate() {
        if *b == LF {
            return TokenStatus::Complete((&buf[..i + 1], &buf[i + 1..]));
        }
    }
    TokenStatus::Partial(buf)
}

// TODO: Replace with `[u8]::trim_ascii_start` when is stable.
fn trim_ascii_start(mut bytes: &[u8]) -> &[u8] {
    // Note: A pattern matching based approach (instead of indexing) allows
    // making the function const.
    while let [first, rest @ ..] = bytes {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

// TODO: Replace with `[u8]::trim_ascii_end` when is stable.
fn trim_ascii_end(mut bytes: &[u8]) -> &[u8] {
    // Note: A pattern matching based approach (instead of indexing) allows
    // making the function const.
    while let [rest @ .., last] = bytes {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

// TODO: Replace with `[u8]::trim_ascii` when is stable.
fn trim_ascii(bytes: &[u8]) -> &[u8] {
    trim_ascii_end(trim_ascii_start(bytes))
}

// get multipart boundary
pub(crate) fn get_content_type_boundary(headers: &Headers) -> Option<Vec<u8>> {
    let header_value = headers.get("Content-Type");
    if let Some(value) = header_value {
        let str = value.to_vec();
        let str = trim_ascii(&str);
        if !str.starts_with(b"multipart") {
            return None;
        }

        let boundary = str
            .split(|b| *b == b';')
            .map(trim_ascii)
            .find(|s| s.starts_with(b"boundary="));

        if let Some(boundary) = boundary {
            let boundary = trim_ascii_start(&boundary[9..]);
            if boundary.len() > 2 && boundary.starts_with(&[b'"']) && boundary.ends_with(&[b'"']) {
                return Some(boundary[1..boundary.len() - 1].to_vec());
            } else if !boundary.is_empty() {
                return Some(boundary[..].to_vec());
            }
        }
    }
    None
}

#[cfg(test)]
mod ut_common {
    use crate::body::mime::common::get_content_type_boundary;
    use crate::headers::Headers;

    /// UT test cases for `get_content_type_boundary`.
    ///
    /// # Brief
    /// 1. Creates a `Headers` and inserts key and value.
    /// 2. Gets boundary from headers by `get_content_type_boundary`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_get_content_type_boundary() {
        // common
        let mut headers = Headers::new();
        headers
            .insert(
                "Content-Type",
                "multipart/mixed; boundary=gc0p4Jq0M2Yt08j34c0p",
            )
            .unwrap();
        assert_eq!(
            get_content_type_boundary(&headers),
            Some(b"gc0p4Jq0M2Yt08j34c0p".to_vec())
        );
        // has LWSF
        let mut headers = Headers::new();
        headers
            .insert(
                "Content-Type",
                "  multipart/mixed; boundary=   gc0p4Jq0M2Yt08j34c0p  ",
            )
            .unwrap();
        assert_eq!(
            get_content_type_boundary(&headers),
            Some(b"gc0p4Jq0M2Yt08j34c0p".to_vec())
        );
        // has ""
        let mut headers = Headers::new();
        headers
            .insert(
                "Content-Type",
                r#"multipart/mixed; boundary="gc0pJq0M:08jU534c0p""#,
            )
            .unwrap();
        assert_eq!(
            get_content_type_boundary(&headers),
            Some(b"gc0pJq0M:08jU534c0p".to_vec())
        );
    }
}
