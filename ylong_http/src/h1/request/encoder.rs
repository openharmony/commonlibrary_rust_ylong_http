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

//! HTTP/1 request encoder implementation.
//!
//! The encoder is used to serialize the request into the specified buf in
//! a certain format.
//!
//! # Examples
//!
//! ```
//! use ylong_http::h1::RequestEncoder;
//! use ylong_http::request::Request;
//!
//! let request = Request::builder()
//!     .method("GET")
//!     .url("www.example.com")
//!     .version("HTTP/1.1")
//!     .header("ACCEPT", "text/html")
//!     .body(())
//!     .unwrap();
//!
//! // Gets `RequestPart`.
//! let (part, _) = request.into_parts();
//! let mut encoder = RequestEncoder::new(part);
//! encoder.absolute_uri(true);
//!
//! // We use `message` to store all the body data.
//! let mut message = Vec::new();
//! // We use `buf` to store save temporary data.
//! let mut buf = [0u8; 20];
//!
//! // First encoding, buf is filled.
//! let size = encoder.encode(&mut buf).unwrap();
//! assert_eq!(&buf[..size], "GET www.example.com ".as_bytes());
//! message.extend_from_slice(&buf[..size]);
//!
//! // Second encoding, buf is filled.
//! let size = encoder.encode(&mut buf).unwrap();
//! assert_eq!(&buf[..size], "HTTP/1.1\r\naccept:tex".as_bytes());
//! message.extend_from_slice(&buf[..size]);
//!
//! // Third encoding, part of buf is filled, this indicates that encoding has ended.
//! let size = encoder.encode(&mut buf).unwrap();
//! assert_eq!(&buf[..size], "t/html\r\n\r\n".as_bytes());
//! message.extend_from_slice(&buf[..size]);
//!
//! // We can assemble temporary data into a complete data.
//! let result = "GET www.example.com HTTP/1.1\r\naccept:text/html\r\n\r\n";
//! assert_eq!(message.as_slice(), result.as_bytes());
//! ```

use std::io::Read;

use crate::error::{ErrorKind, HttpError};
use crate::headers::{HeaderName, Headers, HeadersIntoIter};
use crate::request::method::Method;
use crate::request::uri::Uri;
use crate::request::RequestPart;
use crate::version::Version;

/// A encoder that is used to encode request message in `HTTP/1` format.
///
/// This encoder supports you to use the encode method multiple times to output
/// the result in multiple bytes slices.
///
/// # Examples
///
/// ```
/// use ylong_http::h1::RequestEncoder;
/// use ylong_http::request::Request;
///
/// let request = Request::builder()
///     .method("GET")
///     .url("www.example.com")
///     .version("HTTP/1.1")
///     .header("ACCEPT", "text/html")
///     .body(())
///     .unwrap();
///
/// // Gets `RequestPart`.
/// let (part, _) = request.into_parts();
/// let mut encoder = RequestEncoder::new(part);
/// encoder.absolute_uri(true);
///
/// // We use `message` to store all the body data.
/// let mut message = Vec::new();
/// // We use `buf` to store save temporary data.
/// let mut buf = [0u8; 20];
///
/// // First encoding, buf is filled.
/// let size = encoder.encode(&mut buf).unwrap();
/// assert_eq!(&buf[..size], "GET www.example.com ".as_bytes());
/// message.extend_from_slice(&buf[..size]);
///
/// // Second encoding, buf is filled.
/// let size = encoder.encode(&mut buf).unwrap();
/// assert_eq!(&buf[..size], "HTTP/1.1\r\naccept:tex".as_bytes());
/// message.extend_from_slice(&buf[..size]);
///
/// // Third encoding, part of buf is filled, this indicates that encoding has ended.
/// let size = encoder.encode(&mut buf).unwrap();
/// assert_eq!(&buf[..size], "t/html\r\n\r\n".as_bytes());
/// message.extend_from_slice(&buf[..size]);
///
/// // We can assemble temporary data into a complete data.
/// let result = "GET www.example.com HTTP/1.1\r\naccept:text/html\r\n\r\n";
/// assert_eq!(message.as_slice(), result.as_bytes());
/// ```
pub struct RequestEncoder {
    encode_status: EncodeState,
    method_part: EncodeMethod,
    method_sp_part: EncodeSp,
    uri_part: EncodeUri,
    uri_sp_part: EncodeSp,
    version_part: EncodeVersion,
    version_crlf_part: EncodeCrlf,
    headers_part: EncodeHeader,
    headers_crlf_part: EncodeCrlf,
    is_absolute_uri: bool,
}

enum EncodeState {
    // "Method" phase of encoding request-message.
    Method,
    // "MethodSp" phase of encoding whitespace after method.
    MethodSp,
    // "Uri" phase of encoding request-message.
    Uri,
    // "UriSp" phase of encoding whitespace after uri.
    UriSp,
    // "Version" phase of encoding request-message.
    Version,
    // "VersionCrlf" phase of encoding whitespace after version.
    VersionCrlf,
    // "Header" phase of encoding request-message.
    Header,
    // "HeaderCrlf" phase of encoding /r/n after header.
    HeaderCrlf,
    // "EncodeFinished" phase of finishing the encoding.
    EncodeFinished,
}

// Component encoding status.
enum TokenStatus<T, E> {
    // The current component is completely encoded.
    Complete(T),
    // The current component is partially encoded.
    Partial(E),
}

type TokenResult<T> = Result<TokenStatus<usize, T>, HttpError>;

impl RequestEncoder {
    /// Creates a new `RequestEncoder` from a `RequestPart`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h1::RequestEncoder;
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::builder()
    ///     .method("GET")
    ///     .url("www.example.com")
    ///     .version("HTTP/1.1")
    ///     .header("ACCEPT", "text/html")
    ///     .body(())
    ///     .unwrap();
    ///
    /// let (part, _) = request.into_parts();
    /// let encoder = RequestEncoder::new(part);
    /// ```
    pub fn new(part: RequestPart) -> Self {
        Self {
            encode_status: EncodeState::Method,
            method_part: EncodeMethod::new(part.method),
            method_sp_part: EncodeSp::new(),
            uri_part: EncodeUri::new(part.uri, false),
            uri_sp_part: EncodeSp::new(),
            version_part: EncodeVersion::new(part.version),
            version_crlf_part: EncodeCrlf::new(),
            headers_part: EncodeHeader::new(part.headers),
            headers_crlf_part: EncodeCrlf::new(),
            is_absolute_uri: false,
        }
    }

    /// Encodes `RequestPart` into target buf and returns the number of
    /// bytes written.
    ///
    /// If the length of buf is not enough to write all the output results,
    /// the state will be saved until the next call to this method.
    ///
    /// # Return Value
    ///
    /// This method may return the following results:
    ///
    /// - `Ok(size) && size == buf.len()`: it means that buf has been completely
    /// filled, but the result may not be fully output. You **must** call this
    /// method again to obtain the rest part of the result. Otherwise you may
    /// lose some parts of the result.
    ///
    /// - `Ok(size) && size < buf.len()`: it indicates that the result has been
    /// fully output.
    ///
    /// - `Err(e)`: it indicates that an error has occurred during encoding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h1::RequestEncoder;
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::builder()
    ///     .method("GET")
    ///     .url("www.example.com")
    ///     .version("HTTP/1.1")
    ///     .header("ACCEPT", "text/html")
    ///     .body(())
    ///     .unwrap();
    ///
    /// let (part, _) = request.into_parts();
    /// let mut encoder = RequestEncoder::new(part);
    /// encoder.absolute_uri(true);
    ///
    /// let mut buf = [0_u8; 10];
    /// let mut message = Vec::new();
    /// let mut idx = 0;
    /// loop {
    ///     let size = encoder.encode(&mut buf).unwrap();
    ///     message.extend_from_slice(&buf[..size]);
    ///     if size < buf.len() {
    ///         break;
    ///     }
    /// }
    ///
    /// let result = "GET www.example.com HTTP/1.1\r\naccept:text/html\r\n\r\n";
    /// assert_eq!(message.as_slice(), result.as_bytes());
    /// ```
    pub fn encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        if dst.is_empty() {
            return Err(ErrorKind::InvalidInput.into());
        }
        let mut count = 0;
        while count != dst.len() {
            count += match self.encode_status {
                EncodeState::Method => self.method_encode(&mut dst[count..]),
                EncodeState::MethodSp => self.method_sp_encode(&mut dst[count..]),
                EncodeState::Uri => self.uri_encode(&mut dst[count..]),
                EncodeState::UriSp => self.uri_sp_encode(&mut dst[count..]),
                EncodeState::Version => self.version_encode(&mut dst[count..]),
                EncodeState::VersionCrlf => self.version_crlf_encode(&mut dst[count..]),
                EncodeState::Header => self.header_encode(&mut dst[count..]),
                EncodeState::HeaderCrlf => self.header_crlf_encode(&mut dst[count..]),
                EncodeState::EncodeFinished => return Ok(count),
            }?;
        }
        Ok(dst.len())
    }

    /// Sets the `is_absolute_uri` flag.
    ///
    /// If you enable the flag, the uri part will be encoded as absolute form
    /// in the headline. Otherwise the uri part will be encoded as origin form.
    ///
    /// You should use this method before the uri part being encoded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h1::RequestEncoder;
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::builder()
    ///     .method("GET")
    ///     .url("www.example.com")
    ///     .version("HTTP/1.1")
    ///     .header("ACCEPT", "text/html")
    ///     .body(())
    ///     .unwrap();
    ///
    /// let (part, _) = request.into_parts();
    /// let mut encoder = RequestEncoder::new(part);
    /// // After you create the request encoder, users can choose to set the uri form.
    /// encoder.absolute_uri(false);
    ///
    /// let mut buf = [0u8; 1024];
    /// let size = encoder.encode(&mut buf).unwrap();
    /// // If you disable the `is_absolute_uri` flag, the uri will be encoded as a origin form.
    /// assert_eq!(
    ///     &buf[..size],
    ///     b"GET / HTTP/1.1\r\naccept:text/html\r\n\r\n".as_slice()
    /// );
    /// ```
    pub fn absolute_uri(&mut self, is_absolute: bool) {
        self.is_absolute_uri = is_absolute;
    }

    fn method_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.method_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::MethodSp;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::Method;
                Ok(output_size)
            }
        }
    }

    fn method_sp_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.method_sp_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.uri_part.is_absolute = self.is_absolute_uri;
                self.encode_status = EncodeState::Uri;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::MethodSp;
                Ok(output_size)
            }
        }
    }

    fn uri_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.uri_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::UriSp;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::Uri;
                Ok(output_size)
            }
        }
    }

    fn uri_sp_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.uri_sp_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::Version;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::UriSp;
                Ok(output_size)
            }
        }
    }

    fn version_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.version_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::VersionCrlf;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::Version;
                Ok(output_size)
            }
        }
    }

    fn version_crlf_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.version_crlf_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::Header;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::VersionCrlf;
                Ok(output_size)
            }
        }
    }

    fn header_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.headers_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::HeaderCrlf;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::Header;
                Ok(output_size)
            }
        }
    }

    fn header_crlf_encode(&mut self, dst: &mut [u8]) -> Result<usize, HttpError> {
        match self.headers_crlf_part.encode(dst)? {
            TokenStatus::Complete(output_size) => {
                self.encode_status = EncodeState::EncodeFinished;
                Ok(output_size)
            }
            TokenStatus::Partial(output_size) => {
                self.encode_status = EncodeState::HeaderCrlf;
                Ok(output_size)
            }
        }
    }
}

struct EncodeMethod {
    inner: Method,
    src_idx: usize,
}

impl EncodeMethod {
    fn new(method: Method) -> Self {
        Self {
            inner: method,
            src_idx: 0,
        }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        let method = self.inner.as_str().as_bytes();
        WriteData::new(method, &mut self.src_idx, buf).write()
    }
}

struct EncodeUri {
    absolute: Vec<u8>,
    origin: Vec<u8>,
    src_idx: usize,
    is_absolute: bool,
}

impl EncodeUri {
    fn new(uri: Uri, is_absolute: bool) -> Self {
        let mut origin_form = vec![];
        let path = uri.path_and_query();
        if let Some(p) = path {
            origin_form = p.as_bytes().to_vec();
        } else {
            origin_form.extend_from_slice(b"/");
        }
        let init_uri = uri.to_string().into_bytes();
        Self {
            absolute: init_uri,
            origin: origin_form,
            src_idx: 0,
            is_absolute,
        }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        let mut uri = self.origin.as_slice();
        if self.is_absolute {
            uri = self.absolute.as_slice();
        }
        WriteData::new(uri, &mut self.src_idx, buf).write()
    }
}

struct EncodeVersion {
    inner: Version,
    src_idx: usize,
}

impl EncodeVersion {
    fn new(version: Version) -> Self {
        Self {
            inner: version,
            src_idx: 0,
        }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        let version = self.inner.as_str().as_bytes();
        let mut task = WriteData::new(version, &mut self.src_idx, buf);
        task.write()
    }
}

struct EncodeHeader {
    inner: HeadersIntoIter,
    status: Option<HeaderStatus>,
    name: HeaderName,
    value: Vec<u8>,
    name_idx: usize,
    colon_idx: usize,
    value_idx: usize,
}

enum HeaderStatus {
    Name,
    Colon,
    Value,
    Crlf(EncodeCrlf),
    EmptyHeader,
}

impl EncodeHeader {
    fn new(header: Headers) -> Self {
        let mut header_iter = header.into_iter();
        if let Some((header_name, header_value)) = header_iter.next() {
            Self {
                inner: header_iter,
                status: Some(HeaderStatus::Name),
                name: header_name,
                value: header_value.to_string().unwrap().into_bytes(),
                name_idx: 0,
                colon_idx: 0,
                value_idx: 0,
            }
        } else {
            Self {
                inner: header_iter,
                status: Some(HeaderStatus::EmptyHeader),
                name: HeaderName::from_bytes(" ".as_bytes()).unwrap(),
                value: vec![],
                name_idx: 0,
                colon_idx: 0,
                value_idx: 0,
            }
        }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        match self.status.take().unwrap() {
            HeaderStatus::Name => {
                let name = self.name.as_bytes();
                let mut task = WriteData::new(name, &mut self.name_idx, buf);
                match task.write()? {
                    TokenStatus::Complete(size) => {
                        self.status = Some(HeaderStatus::Colon);
                        Ok(TokenStatus::Partial(size))
                    }
                    TokenStatus::Partial(size) => {
                        self.status = Some(HeaderStatus::Name);
                        Ok(TokenStatus::Partial(size))
                    }
                }
            }
            HeaderStatus::Colon => {
                let colon = ":".as_bytes();
                let mut task = WriteData::new(colon, &mut self.colon_idx, buf);
                match task.write()? {
                    TokenStatus::Complete(size) => {
                        self.status = Some(HeaderStatus::Value);
                        Ok(TokenStatus::Partial(size))
                    }
                    TokenStatus::Partial(size) => {
                        self.status = Some(HeaderStatus::Colon);
                        Ok(TokenStatus::Partial(size))
                    }
                }
            }
            HeaderStatus::Value => {
                let value = self.value.as_slice();
                let mut task = WriteData::new(value, &mut self.value_idx, buf);
                match task.write()? {
                    TokenStatus::Complete(size) => {
                        let crlf = EncodeCrlf::new();
                        self.status = Some(HeaderStatus::Crlf(crlf));
                        Ok(TokenStatus::Partial(size))
                    }
                    TokenStatus::Partial(size) => {
                        self.status = Some(HeaderStatus::Value);
                        Ok(TokenStatus::Partial(size))
                    }
                }
            }
            HeaderStatus::Crlf(mut crlf) => match crlf.encode(buf)? {
                TokenStatus::Complete(size) => {
                    if let Some(iter) = self.inner.next() {
                        let (header_name, header_value) = iter;
                        self.status = Some(HeaderStatus::Name);
                        self.name = header_name;
                        self.value = header_value.to_string().unwrap().into_bytes();
                        self.name_idx = 0;
                        self.colon_idx = 0;
                        self.value_idx = 0;
                        Ok(TokenStatus::Partial(size))
                    } else {
                        Ok(TokenStatus::Complete(size))
                    }
                }
                TokenStatus::Partial(size) => {
                    self.status = Some(HeaderStatus::Crlf(crlf));
                    Ok(TokenStatus::Partial(size))
                }
            },
            HeaderStatus::EmptyHeader => Ok(TokenStatus::Complete(0)),
        }
    }
}

struct EncodeSp {
    src_idx: usize,
}

impl EncodeSp {
    fn new() -> Self {
        Self { src_idx: 0 }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        let sp = " ".as_bytes();
        let mut task = WriteData::new(sp, &mut self.src_idx, buf);
        task.write()
    }
}

struct EncodeCrlf {
    src_idx: usize,
}

impl EncodeCrlf {
    fn new() -> Self {
        Self { src_idx: 0 }
    }

    fn encode(&mut self, buf: &mut [u8]) -> TokenResult<usize> {
        let crlf = "\r\n".as_bytes();
        let mut task = WriteData::new(crlf, &mut self.src_idx, buf);
        task.write()
    }
}

struct WriteData<'a> {
    src: &'a [u8],
    src_idx: &'a mut usize,
    dst: &'a mut [u8],
}

impl<'a> WriteData<'a> {
    fn new(src: &'a [u8], src_idx: &'a mut usize, dst: &'a mut [u8]) -> Self {
        WriteData { src, src_idx, dst }
    }

    fn write(&mut self) -> TokenResult<usize> {
        let src_idx = *self.src_idx;
        let input_len = self.src.len() - src_idx;
        let output_len = self.dst.len();
        let num = (&self.src[src_idx..]).read(self.dst).unwrap();
        if output_len >= input_len {
            return Ok(TokenStatus::Complete(num));
        }
        *self.src_idx += num;
        Ok(TokenStatus::Partial(num))
    }
}
impl Default for RequestEncoder {
    fn default() -> Self {
        RequestEncoder::new(RequestPart::default())
    }
}

#[cfg(test)]
mod ut_request_encoder {
    use super::RequestEncoder;
    use crate::request::{Request, RequestBuilder};

    /// UT test cases for `RequestEncoder::new`.
    ///
    /// # Brief
    /// 1. Calls `RequestEncoder::new` to create a `RequestEncoder`.
    #[test]
    fn ut_request_encoder_new() {
        let request = Request::new(());
        let (part, _) = request.into_parts();
        let _encoder = RequestEncoder::new(part);
        // Success if no panic.
    }

    /// UT test cases for `RequestEncoder::encode`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling methods of `Request::builder`.
    /// 2. Gets a request part by calling `Request::into_parts`.
    /// 3. Creates a `RequestEncoder` by calling `RequestBuilder::new`.
    /// 4. Calls `RequestEncoder::encode` method in a loop and collects the
    ///    results.
    /// 5. Checks if the test result is correct.
    #[test]
    fn ut_request_encoder_encode_1() {
        macro_rules! encoder_test_case {
            (
                Method: $method:expr,
                Uri: $uri:expr,
                Version: $version:expr,
                $(Header: $name:expr, $value:expr,)*
                RequestLine: $request_line:expr,
            ) => {{
                let request = Request::builder()
                    .method($method)
                    .url($uri)
                    .version($version)
                    $(.header($name, $value))*
                    .body(())
                    .unwrap();

                let (part, _) = request.into_parts();
                let mut encoder = RequestEncoder::new(part);
                encoder.absolute_uri(true);
                let mut buf = [0u8; 5];
                let mut res = Vec::new();
                loop {
                    let size = encoder.encode(&mut buf).unwrap();
                    res.extend_from_slice(&buf[..size]);
                    if size < buf.len() {
                        break;
                    }
                }

                let str = std::str::from_utf8(res.as_slice())
                    .expect("Cannot convert res to &str");

                assert!(str.find($request_line).is_some());

                $(
                    let target_header = format!(
                        "{}:{}\r\n",
                        ($name).to_lowercase(),
                        ($value).to_lowercase(),
                    );
                    assert!(str.find(target_header.as_str()).is_some());
                )*
            }};
        }

        // No header-lines.
        encoder_test_case! {
            Method: "GET",
            Uri: "www.example.com",
            Version: "HTTP/1.1",
            RequestLine: "GET www.example.com HTTP/1.1\r\n",
        }

        // 1 header-line.
        encoder_test_case! {
            Method: "GET",
            Uri: "www.example.com",
            Version: "HTTP/1.1",
            Header: "ACCEPT", "text/html",
            RequestLine: "GET www.example.com HTTP/1.1\r\n",
        }

        // More than 1 header-lines.
        encoder_test_case! {
            Method: "GET",
            Uri: "www.example.com",
            Version: "HTTP/1.1",
            Header: "ACCEPT", "text/html",
            Header: "HOST", "127.0.0.1",
            RequestLine: "GET www.example.com HTTP/1.1\r\n",
        }
    }

    /// UT test cases for `RequestEncoder::absolute_uri`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `RequestBuilder::build`.
    /// 2. Calls absolute_uri.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_encoder_set_proxy() {
        let request = RequestBuilder::new()
            .method("GET")
            .url("www.example.com")
            .version("HTTP/1.1")
            .body(())
            .unwrap();
        let (part, _) = request.into_parts();
        let mut encoder = RequestEncoder::new(part);
        assert!(!encoder.is_absolute_uri);

        encoder.absolute_uri(true);
        assert!(encoder.is_absolute_uri);
        encoder.absolute_uri(false);

        let mut buf = [0u8; 100];
        let size = encoder.encode(&mut buf).unwrap();
        let res = std::str::from_utf8(&buf[..size]).unwrap();
        assert_eq!(res, "GET / HTTP/1.1\r\n\r\n");
    }
}
