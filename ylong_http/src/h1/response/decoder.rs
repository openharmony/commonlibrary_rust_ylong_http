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

use crate::error::{ErrorKind, HttpError};
use crate::h1::H1Error;
use crate::headers::Headers;
use crate::response::status::StatusCode;
use crate::response::ResponsePart;
use crate::util::header_bytes::{HEADER_NAME_BYTES, HEADER_VALUE_BYTES};
use crate::version::Version;

/// `HTTP/1` response decoder, which support decoding multi-segment byte
/// streams into `Response`.
///
/// # Examples
///
/// ```
/// use ylong_http::h1::ResponseDecoder;
/// use ylong_http::version::Version;
///
/// // The complete message is:
/// // "HTTP/1.1 304 OK\r\nContent-Length:4\r\n\r\nbody"
/// let strings = ["HTTP/1.1 304 OK\r\nCon", "tent-Length:", "4\r\n\r\nbody"];
///
/// // We need to create a decoder first.
/// let mut decoder = ResponseDecoder::new();
///
/// // Then we use it to decode some bytes.
/// // The first part of bytes is correct, but we need more bytes to get a `ResponsePart`.
/// assert_eq!(decoder.decode(strings[0].as_bytes()), Ok(None));
/// // The second part is also correct, but we still need more bytes.
/// assert_eq!(decoder.decode(strings[1].as_bytes()), Ok(None));
/// // After decoding the third part, a complete `ResponsePart` is decoded.
/// let (part, body) = decoder.decode(strings[2].as_bytes()).unwrap().unwrap();
///
/// // Then we can use the decode result.
/// assert_eq!(part.version.as_str(), "HTTP/1.1");
/// assert_eq!(part.status.as_u16(), 304);
/// assert_eq!(
///     part.headers
///         .get("content-length")
///         .unwrap()
///         .to_string()
///         .unwrap(),
///     "4"
/// );
/// assert_eq!(body, b"body");
/// ```
pub struct ResponseDecoder {
    // Parsing phase, corresponding to each component of response-message.
    stage: ParseStage,
    version: Option<Version>,
    status_code: Option<StatusCode>,
    headers: Option<Headers>,
    // Cache the parsed header key.
    head_key: Vec<u8>,
    // Cache the response-message component whose current bytes segment is incomplete
    rest: Vec<u8>,
    // The value is true when the last byte of the current byte segment is CR.
    new_line: bool,
}

// Component parsing status
enum TokenStatus<T, E> {
    // The current component is completely parsed.
    Complete(T),
    // The current component is not completely parsed.
    Partial(E),
}

// ResponseDecoder parsing phase, All components of response-message are as
// follows:
// ---------------------------------------------------------
// | HTTP-version SP status-code SP [ reason-phrase ]CRLF  | // status-line
// | *( field-name ":" OWS field-value OWS CRLF )          | // field-line
// | CRLF                                                  |
// | [message-body ]                                       |
// ---------------------------------------------------------
#[derive(Clone)]
enum ParseStage {
    // Decoder initialization phase, The decoder parses the bytes for the first time.
    Initial,
    // "HTTP-version" phase of parsing response-message
    Version,
    // "status-code" phase of parsing response-message
    StatusCode,
    // "reason-phrase" phase of parsing response-message
    Reason,
    // CRLF after "reason-phrase" of parsing response-message
    StatusCrlf,
    // "field-line" phase of parsing response-message
    Header(HeaderStage),
    // CRLF after "field-line" of parsing response-message
    BlankLine,
}

// Stage of parsing field-line, the filed line component is as follows:
// ------------------------------------------------
// | *( field-name ":" OWS field-value OWS CRLF )  |
// ------------------------------------------------
#[derive(Clone)]
enum HeaderStage {
    // Check whether the response-message contains field-line.
    Start,
    Key,
    // OWS phase before "field-value"
    OwsBefore,
    Value,
    Crlf,
    // After a filed-line line is parsed, the parsing phase is EndCrlf.
    // In this case, you need to check whether all field lines are ended.
    EndCrlf,
}

impl Default for ResponseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

type TokenResult<'a> = Result<TokenStatus<(&'a [u8], &'a [u8]), &'a [u8]>, HttpError>;

macro_rules! get_unparsed_or_return {
    ($case:expr, $buffer:expr) => {{
        match $case {
            Some(unparsed) => $buffer = unparsed,
            None => return Ok(None),
        }
    }};
}

macro_rules! detect_blank_and_return {
    ($case:expr, $buffer:expr) => {{
        if $buffer.is_empty() {
            $case = ParseStage::Header(HeaderStage::EndCrlf);
            return Ok(None);
        } else if $buffer[0] == b'\r' || $buffer[0] == b'\n' {
            return Ok(Some($buffer));
        }
    }};
}

impl ResponseDecoder {
    /// Creates a new `ResponseDecoder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h1::ResponseDecoder;
    ///
    /// let decoder = ResponseDecoder::new();
    /// ```
    pub fn new() -> Self {
        ResponseDecoder {
            stage: ParseStage::Initial,
            version: None,
            status_code: None,
            headers: None,
            head_key: vec![],
            rest: vec![],
            new_line: false,
        }
    }

    /// Decodes some bytes to get a complete `ResponsePart`. This method can be
    /// invoked multiple times util a complete `ResponsePart` is returned.
    ///
    /// Only status line and field line is decoded. The body of `Response` is
    /// not be decoded.
    ///
    /// Returns `Ok(None)` if decoder needs more bytes to decode.
    ///
    /// Returns `ResponsePart` and the remaining bytes if decoder has complete
    /// decoding. The remaining bytes will be returned as a slice.
    ///
    /// Returns `Err` if the input is not syntactically valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h1::ResponseDecoder;
    /// use ylong_http::version::Version;
    ///
    /// let valid = ["HTTP/1.1", " 304 OK\r\n\r\n"];
    /// let mut decoder = ResponseDecoder::new();
    /// // Returns `Ok(None)` if decoder needs more bytes to decode.
    /// assert_eq!(decoder.decode(valid[0].as_bytes()), Ok(None));
    /// // Returns `ResponsePart` and a slice of bytes if decoder has complete decoding.
    /// let (part, body) = decoder.decode(valid[1].as_bytes()).unwrap().unwrap();
    /// assert_eq!(part.version, Version::HTTP1_1);
    /// assert_eq!(part.status.as_u16(), 304);
    /// assert!(part.headers.is_empty());
    /// assert!(body.is_empty());
    ///
    /// // Returns `Err` if the input is not syntactically valid.
    /// let mut decoder = ResponseDecoder::new();
    /// let invalid_str = "invalid str".as_bytes();
    /// assert!(decoder.decode(invalid_str).is_err());
    /// ```
    pub fn decode<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        match self.stage {
            ParseStage::Initial => self.version_phase(buf),
            ParseStage::Version => self.version_phase(buf),
            ParseStage::StatusCode => self.status_code_phase(buf),
            ParseStage::Reason => self.reason_phase(buf),
            ParseStage::StatusCrlf => self.status_crlf_phase(buf),
            ParseStage::Header(_) => self.header_phase(buf),
            ParseStage::BlankLine => self.blank_line_phase(buf),
        }
    }

    fn version_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        self.stage = ParseStage::Version;
        match status_token(buffer)? {
            TokenStatus::Complete((version, unparsed)) => {
                let version = self.take_value(version);
                match version.as_slice() {
                    b"HTTP/1.0" => {
                        self.version = Some(Version::HTTP1_0);
                    }
                    b"HTTP/1.1" => {
                        self.version = Some(Version::HTTP1_1);
                    }
                    // TODO: Support for other `HTTP` versions.
                    _ => return Err(ErrorKind::H1(H1Error::InvalidResponse).into()),
                }
                self.status_code_phase(unparsed)
            }
            TokenStatus::Partial(rest) => {
                self.rest.extend_from_slice(rest);
                Ok(None)
            }
        }
    }

    fn status_code_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        self.stage = ParseStage::StatusCode;
        match status_token(buffer)? {
            TokenStatus::Complete((code, unparsed)) => {
                let code = self.take_value(code);
                self.status_code = Some(
                    StatusCode::from_bytes(code.as_slice())
                        .map_err(|_| HttpError::from(ErrorKind::H1(H1Error::InvalidResponse)))?,
                );
                self.reason_phase(unparsed)
            }
            TokenStatus::Partial(rest) => {
                self.rest.extend_from_slice(rest);
                Ok(None)
            }
        }
    }

    fn reason_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        self.stage = ParseStage::Reason;
        match decode_reason(buffer)? {
            Some(unparsed) => self.status_crlf_phase(unparsed),
            None => Ok(None),
        }
    }

    fn status_crlf_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        self.stage = ParseStage::StatusCrlf;
        match self.decode_status_crlf(buffer)? {
            Some(unparsed) => self.header_phase_with_init(unparsed),
            None => Ok(None),
        }
    }

    fn header_phase_with_init<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        let headers = Headers::new();
        self.headers = Some(headers);
        if buffer.is_empty() {
            self.stage = ParseStage::Header(HeaderStage::Start);
            return Ok(None);
        } else if buffer[0] == b'\r' || buffer[0] == b'\n' {
            return self.blank_line_phase(buffer);
        } else {
            self.stage = ParseStage::Header(HeaderStage::Key);
        }
        match self.decode_header(buffer)? {
            Some(unparsed) => self.blank_line_phase(unparsed),
            None => Ok(None),
        }
    }

    fn header_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        match self.decode_header(buffer)? {
            Some(unparsed) => self.blank_line_phase(unparsed),
            None => Ok(None),
        }
    }

    fn blank_line_phase<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<(ResponsePart, &'a [u8])>, HttpError> {
        self.stage = ParseStage::BlankLine;
        match self.decode_status_crlf(buffer)? {
            Some(unparsed) => {
                let response_part = ResponsePart {
                    version: self.version.take().unwrap(),
                    status: self.status_code.take().unwrap(),
                    headers: self.headers.take().unwrap(),
                };
                Ok(Some((response_part, unparsed)))
            }
            None => Ok(None),
        }
    }

    fn decode_status_crlf<'a>(&mut self, buffer: &'a [u8]) -> Result<Option<&'a [u8]>, HttpError> {
        match consume_crlf(buffer, take(&mut self.new_line))? {
            TokenStatus::Complete(unparsed) => {
                self.new_line = false;
                Ok(Some(unparsed))
            }
            TokenStatus::Partial(0) => Ok(None),
            TokenStatus::Partial(1) => {
                self.new_line = true;
                Ok(None)
            }
            _ => Err(ErrorKind::H1(H1Error::InvalidResponse).into()),
        }
    }

    fn decode_header<'a>(&mut self, buffer: &'a [u8]) -> Result<Option<&'a [u8]>, HttpError> {
        return match &self.stage {
            ParseStage::Header(header_stage) => match header_stage {
                HeaderStage::Start => {
                    if buffer.is_empty() {
                        return Ok(None);
                    } else if buffer[0] == b'\r' || buffer[0] == b'\n' {
                        return Ok(Some(buffer));
                    } else {
                        self.decode_header_from_key(buffer)
                    }
                }
                HeaderStage::Key => self.decode_header_from_key(buffer),
                HeaderStage::OwsBefore => self.decode_header_from_ows_before(buffer),
                HeaderStage::Value => self.decode_header_from_value(buffer),
                HeaderStage::Crlf => self.decode_header_from_crlf(buffer),
                HeaderStage::EndCrlf => self.decode_header_from_crlf_end(buffer),
            },
            _ => Err(ErrorKind::H1(H1Error::InvalidResponse).into()),
        };
    }

    fn decode_header_from_value<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        let mut buffer = buffer;
        loop {
            get_unparsed_or_return!(self.decode_value(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_crlf(false, buffer)?, buffer);
            detect_blank_and_return!(self.stage, buffer);
            get_unparsed_or_return!(self.decode_key(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_ows(buffer)?, buffer);
        }
    }

    fn decode_header_from_key<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        let mut buffer = buffer;
        loop {
            get_unparsed_or_return!(self.decode_key(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_ows(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_value(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_crlf(false, buffer)?, buffer);
            detect_blank_and_return!(self.stage, buffer);
        }
    }

    fn decode_header_from_ows_before<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        let mut buffer = buffer;
        loop {
            get_unparsed_or_return!(self.decode_ows(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_value(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_crlf(false, buffer)?, buffer);
            detect_blank_and_return!(self.stage, buffer);
            get_unparsed_or_return!(self.decode_key(buffer)?, buffer);
        }
    }

    fn decode_header_from_crlf<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        let mut buffer = buffer;
        loop {
            get_unparsed_or_return!(self.decode_crlf(false, buffer)?, buffer);
            detect_blank_and_return!(self.stage, buffer);
            get_unparsed_or_return!(self.decode_key(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_ows(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_value(buffer)?, buffer);
        }
    }

    fn decode_header_from_crlf_end<'a>(
        &mut self,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        let mut buffer = buffer;
        loop {
            detect_blank_and_return!(self.stage, buffer);
            get_unparsed_or_return!(self.decode_key(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_ows(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_value(buffer)?, buffer);
            get_unparsed_or_return!(self.decode_crlf(false, buffer)?, buffer);
        }
    }

    fn decode_ows<'a>(&mut self, buffer: &'a [u8]) -> Result<Option<&'a [u8]>, HttpError> {
        self.stage = ParseStage::Header(HeaderStage::OwsBefore);
        trim_ows(buffer)
    }

    fn decode_key<'a>(&mut self, buffer: &'a [u8]) -> Result<Option<&'a [u8]>, HttpError> {
        self.stage = ParseStage::Header(HeaderStage::Key);
        match get_header_name(buffer)? {
            TokenStatus::Complete((key, unparsed)) => {
                if !self.rest.is_empty() {
                    self.rest.extend_from_slice(key);
                    let key = take(&mut self.rest);
                    self.head_key = key;
                } else {
                    self.head_key = key.to_vec();
                }
                Ok(Some(unparsed))
            }
            TokenStatus::Partial(rest) => {
                self.rest.extend_from_slice(rest);
                Ok(None)
            }
        }
    }

    // TODO: Try use `&[u8]` instead of `Vec<u8>`.
    fn take_value(&mut self, value: &[u8]) -> Vec<u8> {
        if !self.rest.is_empty() {
            self.rest.extend_from_slice(value);
            take(&mut self.rest)
        } else {
            value.to_vec()
        }
    }

    fn decode_value<'a>(&mut self, buffer: &'a [u8]) -> Result<Option<&'a [u8]>, HttpError> {
        self.stage = ParseStage::Header(HeaderStage::Value);
        match get_header_value(buffer)? {
            TokenStatus::Complete((value, unparsed)) => {
                let complete_value = self.take_value(value);
                let header_value = if let Some(last_visible) = complete_value
                    .iter()
                    .rposition(|b| *b != b' ' && *b != b'\t')
                {
                    complete_value[..last_visible + 1].to_vec()
                } else {
                    // Return value even it is empty.
                    Vec::new()
                };
                self.headers = header_insert(
                    take(&mut self.head_key),
                    header_value,
                    self.headers.take().unwrap(),
                )?;
                Ok(Some(unparsed))
            }
            TokenStatus::Partial(rest) => {
                self.rest.extend_from_slice(rest);
                Ok(None)
            }
        }
    }

    fn decode_crlf<'a>(
        &mut self,
        cr_meet: bool,
        buffer: &'a [u8],
    ) -> Result<Option<&'a [u8]>, HttpError> {
        self.stage = ParseStage::Header(HeaderStage::Crlf);
        match consume_crlf(buffer, cr_meet)? {
            TokenStatus::Complete(unparsed) => {
                self.new_line = false;
                Ok(Some(unparsed))
            }
            TokenStatus::Partial(step) => {
                if step == 1 {
                    self.new_line = true;
                }
                Ok(None)
            }
        }
    }
}

fn status_token(buffer: &[u8]) -> TokenResult {
    for (i, &b) in buffer.iter().enumerate() {
        if b == b' ' {
            return Ok(TokenStatus::Complete((&buffer[..i], &buffer[i + 1..])));
        } else if !is_valid_byte(b) {
            return Err(ErrorKind::H1(H1Error::InvalidResponse).into());
        }
    }
    Ok(TokenStatus::Partial(buffer))
}

fn decode_reason(buffer: &[u8]) -> Result<Option<&[u8]>, HttpError> {
    for (i, b) in buffer.iter().enumerate() {
        if *b == b'\r' || *b == b'\n' {
            return Ok(Some(&buffer[i..]));
        } else if !is_legal_reason_byte(*b) {
            return Err(ErrorKind::H1(H1Error::InvalidResponse).into());
        }
    }
    Ok(None)
}

fn consume_crlf(buffer: &[u8], cr_meet: bool) -> Result<TokenStatus<&[u8], usize>, HttpError> {
    if buffer.is_empty() {
        return Ok(TokenStatus::Partial(0));
    }
    match buffer[0] {
        b'\r' => {
            if cr_meet {
                Err(ErrorKind::H1(H1Error::InvalidResponse).into())
            } else if buffer.len() == 1 {
                Ok(TokenStatus::Partial(1))
            } else if buffer[1] == b'\n' {
                Ok(TokenStatus::Complete(&buffer[2..]))
            } else {
                Err(ErrorKind::H1(H1Error::InvalidResponse).into())
            }
        }
        b'\n' => Ok(TokenStatus::Complete(&buffer[1..])),
        _ => Err(ErrorKind::H1(H1Error::InvalidResponse).into()),
    }
}

fn get_header_name(buffer: &[u8]) -> TokenResult {
    for (i, b) in buffer.iter().enumerate() {
        if *b == b':' {
            return Ok(TokenStatus::Complete((&buffer[..i], &buffer[i + 1..])));
        } else if !HEADER_NAME_BYTES[*b as usize] {
            return Err(ErrorKind::H1(H1Error::InvalidResponse).into());
        }
    }
    Ok(TokenStatus::Partial(buffer))
}

fn get_header_value(buffer: &[u8]) -> TokenResult {
    for (i, b) in buffer.iter().enumerate() {
        if *b == b'\r' || *b == b'\n' {
            return Ok(TokenStatus::Complete((&buffer[..i], &buffer[i..])));
        } else if !HEADER_VALUE_BYTES[*b as usize] {
            return Err(ErrorKind::H1(H1Error::InvalidResponse).into());
        }
    }
    Ok(TokenStatus::Partial(buffer))
}

fn trim_ows(buffer: &[u8]) -> Result<Option<&[u8]>, HttpError> {
    for (i, &b) in buffer.iter().enumerate() {
        match b {
            b' ' | b'\t' => {}
            _ => return Ok(Some(&buffer[i..])),
        }
    }
    Ok(None)
}

fn header_insert(
    header_name: Vec<u8>,
    header_value: Vec<u8>,
    mut headers: Headers,
) -> Result<Option<Headers>, HttpError> {
    let name = unsafe { String::from_utf8_unchecked(header_name) };
    let value = unsafe { String::from_utf8_unchecked(header_value) };
    // TODO: Convert `HeaderName` to lowercase when decoding it.
    let key = name.to_lowercase();
    let header_name = key.as_str();
    let header_value = value.as_str();
    // If the response contains headers with the same name, add them to one
    // `Headers`.
    headers.append(header_name, header_value)?;
    Ok(Some(headers))
}

//  token = 1*tchar
//  tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" /
//  "-" / "." / "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
fn is_valid_byte(byte: u8) -> bool {
    byte > 0x1F && byte < 0x7F
}

// reason-phase = 1*(HTAB / SP / VCHAR / obs-text)
fn is_legal_reason_byte(byte: u8) -> bool {
    byte == 0x09 || byte == 0x20 || (0x21..=0x7E).contains(&byte) || (0x80..=0xFF).contains(&byte)
}

// TODO: Add more test cases.
#[cfg(test)]
mod ut_decoder {
    use super::{H1Error, ResponseDecoder};
    use crate::error::{ErrorKind, HttpError};

    macro_rules! test_unit_complete {
        ($res1:expr, $res2:expr, $res3:expr, $res4:expr, $res5:expr) => {{
            let mut decoder = ResponseDecoder::new();
            let result = decoder.decode($res1).unwrap().unwrap();
            assert_eq!($res2, result.0.version.as_str());
            assert_eq!($res3, result.0.status.as_u16());
            assert_eq!($res4.len(), result.0.headers.len());
            for (key, value) in $res4 {
                assert_eq!(
                    value,
                    result.0.headers.get(key).unwrap().to_string().unwrap()
                )
            }
            assert_eq!($res5, result.1);
        }};
    }

    macro_rules! test_unit_segment {
        ($res1:expr, $res2:expr, $res3:expr, $res4:expr, $res5:expr) => {{
            let mut decoder = ResponseDecoder::new();
            let result = decoder.decode($res1.0).unwrap();
            assert_eq!(true, result.is_none());
            let result = decoder.decode($res1.1).unwrap().unwrap();
            assert_eq!($res2, result.0.version.as_str());
            assert_eq!($res3, result.0.status.as_u16());
            assert_eq!($res4.len(), result.0.headers.len());
            for (key, value) in $res4 {
                assert_eq!(
                    value,
                    result.0.headers.get(key).unwrap().to_string().unwrap()
                )
            }
            assert_eq!($res5, result.1);
        }};
    }

    macro_rules! test_unit_invalid {
        ($res1:expr, $res2:expr) => {{
            let mut decoder = ResponseDecoder::new();
            let result = decoder.decode($res1);
            assert_eq!($res2, result.err());
        }};
    }

    /// UT test cases for `ResponseDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ResponseDecoder` by calling `ResponseDecoder::new`.
    /// 2. Decodes response bytes by calling `ResponseDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_decoder_decode_0() {
        // Decode a complete response separated by CRLF.
        test_unit_complete!(
            "HTTP/1.1 304 OK\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        // Decode a complete response separated by LF.
        test_unit_complete!(
            "HTTP/1.1 304 OK\nAge:270646\nDate:Mon, 19 Dec 2022 01:46:59 GMT\nEtag:\"3147526947+gzip\"\n\nbody part".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        // Decode a response without reason-phrase.
        test_unit_complete!(
            "HTTP/1.1 304 \r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );

        // Decode a response that contains the OWS.
        test_unit_complete!(
            "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        // Decode a response without a message-body
        test_unit_complete!(
            "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#""#.as_bytes()
        );
        // Decode has multiple set-cookie responses.
        test_unit_complete!(
            "HTTP/1.1 304 \r\nSet-Cookie: \t template=; Path=/; Domain=example.com; Expires=Mon, 19 Dec 2022 12:58:54 UTC \t \t\r\n\
        Set-Cookie: \t ezov=06331; Path=/; Domain=example.com; Expires=Mon, 19 Dec 2022 12:58:54 UTC \t \t\r\n\r\n".as_bytes(),
            "HTTP/1.1",
            304_u16,
           [("set-cookie", "template=; Path=/; Domain=example.com; Expires=Mon, 19 Dec 2022 12:58:54 UTC, ezov=06331; Path=/; Domain=example.com; Expires=Mon, 19 Dec 2022 12:58:54 UTC")],
            r#""#.as_bytes()
        );
        // Decode a response without a header.
        test_unit_complete!(
            "HTTP/1.1 304 \r\n\r\n".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [] as [(&str, &str); 0],
            r#""#.as_bytes()
        );
        // Decode a response without a header and separated by LF.
        test_unit_complete!(
            "HTTP/1.1 304 \n\n".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [] as [(&str, &str); 0],
            r#""#.as_bytes()
        );
        // Decode a response with a header and an empty value.
        test_unit_complete!(
            "HTTP/1.1 304 \r\nempty_header: \r\n\r\n".as_bytes(),
            "HTTP/1.1",
            304_u16,
            [("empty_header", "")],
            r#""#.as_bytes()
        );
        // Decode a response with a header and an empty value.
        test_unit_complete!(
            "HTTP/1.0 304 \r\nempty_header: \r\n\r\n".as_bytes(),
            "HTTP/1.0",
            304_u16,
            [("empty_header", "")],
            r#""#.as_bytes()
        );
    }

    /// UT test cases for `ResponseDecoder::decode`.
    ///
    /// # Brief
    /// Decode a segmented transmission response and test `ParseStage` parsing
    /// rules.
    /// 1. Creates a `ResponseDecoder` by calling `ResponseDecoder::new`.
    /// 2. Decodes response bytes by calling `ResponseDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_decoder_decode_1() {
        test_unit_segment!(
            ("HT".as_bytes(), "TP/1.1 304 OK\r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 3".as_bytes(), "04 OK\r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 O".as_bytes(), "K\r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r".as_bytes(), "\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nA".as_bytes(), "ge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: ".as_bytes(), "\t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270".as_bytes(), "646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t".as_bytes(), " \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t \t\r".as_bytes(), "\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t \t\r\n".as_bytes(), "Date: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t \t\r\nDa".as_bytes(), "te: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n".as_bytes(), "\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r".as_bytes(), "\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [("age", "270646"), ("date", "Mon, 19 Dec 2022 01:46:59 GMT"), ("etag", r#""3147526947+gzip""#)],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\n".as_bytes(), "\r\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [] as [(&str, &str); 0],
            r#"body part"#.as_bytes()
        );
        test_unit_segment!(
            ("HTTP/1.1 304 OK\r\n".as_bytes(), "\nbody part".as_bytes()),
            "HTTP/1.1",
            304_u16,
            [] as [(&str, &str); 0],
            r#"body part"#.as_bytes()
        );
    }

    /// UT test cases for `ResponseDecoder::decode`.
    ///
    /// # Brief
    /// Decode an incorrect response bytes.
    /// 1. Creates a `ResponseDecoder` by calling `ResponseDecoder::new`.
    /// 2. Decodes response bytes by calling `ResponseDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_decoder_decode_2() {
        test_unit_invalid!("HTTP/1.2 304 OK\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 3040 OK\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 3 4 OK\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 \0K\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\r\nAge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nA;ge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nA;ge:270646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nAge:270\r646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nAge:270\r646\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nAge:270646\r\r\nDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nAge:270646\r\n\rDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
        test_unit_invalid!("HTTP/1.1 304 OK\r\nAge:270646\r\n\rDate:Mon, 19 Dec 2022 01:46:59 GMT\r\nEtag:\"3147526947+gzip\"\r\n\r\r\nbody part".as_bytes(), Some(HttpError::from(ErrorKind::H1(H1Error::InvalidResponse))));
    }
}
