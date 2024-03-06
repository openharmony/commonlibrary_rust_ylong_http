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
use core::mem::take;
use std::io::Read;

use crate::body::mime::{MixFrom, CR, LF};
use crate::error::HttpError;
use crate::headers::{Header, HeaderName, HeaderValue, Headers};
use crate::AsyncRead;

/// `MimePart` is a body part of a Composite MIME body which is defined in
/// [`RFC2046`]:  The body must then contain one or more body parts, each
/// preceded by a boundary delimiter line, and the last one followed by a
/// closing boundary delimiter line. Each body part then consists of a header
/// area, a blank line, and a body area.
///
/// `MimePart` can be built by [`MimePartBuilder`], then sets headers and body.
///
/// [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1
/// [`MimePartBuilder`]: MimePartBuilder
///
/// # Examples
/// ```
/// use ylong_http::body::MimePart;
///
/// let part = MimePart::builder()
///     .header("accept", "text/html")
///     .body_from_bytes(b"9876543210\r\n")
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Default, PartialEq)]
pub struct MimePart<'a> {
    // |========================================================================
    // | Headers [CRLF Field-body]                                             |
    // |========================================================================
    // if it has body, it has CRLF.
    //
    // [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1
    // body-part := MIME-part-headers [CRLF *OCTET]
    //               ; Lines in a body-part must not start
    //               ; with the specified dash-boundary and
    //               ; the delimiter must not appear anywhere
    //               ; in the body part.  Note that the
    //               ; semantics of a body-part differ from
    //               ; the semantics of a message, as
    //               ; described in the text.
    //
    // OCTET := <any 0-255 octet value>
    pub(crate) headers: Headers,
    // all use for encode; owned use for decode
    pub(crate) body: MixFrom<'a>,
}

impl<'a> MimePart<'a> {
    pub(crate) fn new() -> Self {
        Self {
            headers: Headers::new(),
            body: MixFrom::new(),
        }
    }

    pub(crate) fn set_headers(&mut self, headers: Headers) {
        self.headers = headers;
    }

    pub(crate) fn header<N, V>(&mut self, name: N, value: V) -> Result<&mut Headers, HttpError>
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.headers.insert(name, value)?;
        Ok(self.headers_mut())
    }

    pub(crate) fn append_header<N, V>(
        &mut self,
        name: N,
        value: V,
    ) -> Result<&mut Headers, HttpError>
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.headers.append(name, value)?;
        Ok(self.headers_mut())
    }

    pub(crate) fn body_from_owned(&mut self, data: Vec<u8>) {
        self.body.set_owned(data);
    }

    pub(crate) fn body_from_bytes(&mut self, data: &'a [u8]) {
        self.body.set_bytes(data);
    }

    pub(crate) fn body_from_reader<T>(&mut self, data: T)
    where
        T: Read + Send + Sync + 'static,
    {
        self.body.set_reader(data);
    }

    pub(crate) fn body_from_async_reader<T>(&mut self, data: T)
    where
        T: AsyncRead + Send + Sync + Unpin + 'static,
    {
        self.body.set_async_reader(data);
    }

    pub(crate) fn body_extend_from_slice(&mut self, src: &[u8]) {
        if let MixFrom::Owned { bytes, index: _ } = &mut self.body {
            bytes.extend_from_slice(src)
        }
    }

    /// Keeps the first len elements and discard the others.
    pub(crate) fn body_truncate(&mut self, len: usize) {
        if let MixFrom::Owned { bytes, index: _ } = &mut self.body {
            bytes.truncate(len);
        }
    }

    /// Trims the back '\n' or '\r\n' once.
    pub(crate) fn body_trim_crlf_once(&mut self) {
        if let MixFrom::Owned { bytes, index: _ } = &mut self.body {
            if bytes.ends_with(&[LF]) {
                bytes.pop();
            }
            if bytes.ends_with(&[CR]) {
                bytes.pop();
            }
        }
    }

    /// Creates a [`MimePartBuilder`].
    ///
    /// [`MimePartBuilder`]: MimePartBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePart;
    ///
    /// let part = MimePart::builder().build().unwrap();
    /// ```
    pub fn builder() -> MimePartBuilder<'a> {
        MimePartBuilder::new()
    }

    /// Gets the reference of headers.
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Gets the mutable reference of headers.
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.headers
    }

    /// Gets the ownership of headers by replacing it with an default value.
    pub fn headers_once(&mut self) -> Headers {
        take(&mut self.headers)
    }

    /// Gets the reference of body.
    pub fn body(&self) -> Option<&Vec<u8>> {
        match &self.body {
            MixFrom::Owned { bytes, index: _ } => Some(bytes),
            _ => None,
        }
    }

    /// Gets the mutable reference of body.
    pub fn body_mut(&mut self) -> Option<&mut Vec<u8>> {
        match &mut self.body {
            MixFrom::Owned { bytes, index: _ } => Some(bytes),
            _ => None,
        }
    }

    /// Gets the ownership of body by replacing it with an default value.
    pub fn body_once(&mut self) -> Option<Vec<u8>> {
        match &mut self.body {
            MixFrom::Owned { bytes, index: _ } => Some(take(bytes)),
            _ => None,
        }
    }
}

/// `MimePartBuilder` can set a body part of a Composite MIME body [`MimePart`].
/// `MimePartBuilder` can set headers and body, then builds a [`MimePart`].
///
/// [`MimePart`]: MimePart
///
/// # Examples
/// ```
/// use ylong_http::body::MimePartBuilder;
///
/// let part = MimePartBuilder::new()
///     .header("accept", "text/html")
///     .body_from_bytes(b"9876543210\r\n")
///     .build()
///     .unwrap();
/// ```
pub struct MimePartBuilder<'a> {
    inner: Result<MimePart<'a>, HttpError>,
}

impl<'a> MimePartBuilder<'a> {
    /// Creates a new [`MimePartBuilder`].
    ///
    /// [`MimePartBuilder`]: MimePartBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new();
    /// ```
    pub fn new() -> Self {
        MimePartBuilder {
            inner: Ok(MimePart::new()),
        }
    }

    /// Sets headers of [`MimePartBuilder`].
    ///
    /// [`MimePartBuilder`]: MimePartBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    /// use ylong_http::headers::Headers;
    ///
    /// let part = MimePartBuilder::new().set_headers({
    ///     let mut headers = Headers::new();
    ///     headers.append("accept", "text/html").unwrap();
    ///     headers.append("key1", "value1").unwrap();
    ///     headers
    /// });
    /// ```
    pub fn set_headers(mut self, headers: Headers) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.set_headers(headers);
            inner
        });
        self
    }

    /// Inserts header to the MIME body part.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new()
    ///     .header("accept", "text/html")
    ///     .header("accept", "text/plain");
    /// ```
    pub fn header<N, V>(mut self, name: N, value: V) -> Self
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.inner = self.inner.and_then(move |mut inner| {
            inner.header(name, value)?;
            Ok(inner)
        });
        self
    }

    /// Appends header to the MIME body part.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new()
    ///     .append_header("accept", "text/html")
    ///     .append_header("accept", "text/plain");
    /// ```
    pub fn append_header<N, V>(mut self, name: N, value: V) -> Self
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.inner = self.inner.and_then(move |mut inner| {
            inner.append_header(name, value)?;
            Ok(inner)
        });
        self
    }

    /// Sets body to the MIME body part, the read content is from `Vec<u8>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new().body_from_owned(b"123456".to_vec());
    /// ```
    pub fn body_from_owned(mut self, data: Vec<u8>) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.body_from_owned(data);
            inner
        });
        self
    }

    /// Sets body to the MIME body part, the read content is from memory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new().body_from_bytes("123456".as_bytes());
    /// ```
    pub fn body_from_bytes(mut self, data: &'a [u8]) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.body_from_bytes(data);
            inner
        });
        self
    }

    /// Sets body to the MIME body part, the read content is from a synchronous
    /// reader.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new().body_from_reader("123456".as_bytes());
    /// ```
    pub fn body_from_reader<T>(mut self, data: T) -> Self
    where
        T: Read + Send + Sync + 'static,
    {
        self.inner = self.inner.map(move |mut inner| {
            inner.body_from_reader(data);
            inner
        });
        self
    }

    /// Sets body to the MIME body part, the read content is from a asynchronous
    /// reader.
    pub fn body_from_async_reader<T>(mut self, data: T) -> Self
    where
        T: AsyncRead + Send + Sync + Unpin + 'static,
    {
        self.inner = self.inner.map(move |mut inner| {
            inner.body_from_async_reader(data);
            inner
        });
        self
    }

    /// Builds a [`MimePart`].
    ///
    /// [`MimePart`]: MimePart
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimePartBuilder;
    ///
    /// let part = MimePartBuilder::new().build();
    /// ```
    pub fn build(self) -> Result<MimePart<'a>, HttpError> {
        self.inner
    }
}

impl<'a> Default for MimePartBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PartStatus {
    Start,
    Headers,
    Crlf,
    Body,
    End,
}

#[cfg(test)]
mod ut_mime_part {
    use crate::body::{MimePart, MimePartBuilder};
    use crate::headers::Headers;

    /// UT test cases for `MimePartBuilder::new`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePartBuilder::new`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_builder_new() {
        let part_builder = MimePartBuilder::new();
        assert!(part_builder.inner.is_ok());
    }

    /// UT test cases for `MimePartBuilder::set_headers`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder`.
    /// 2. Sets headers.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_builder_set_headers() {
        let part = MimePartBuilder::new().set_headers({
            let mut headers = Headers::new();
            headers.append("accept", "text/html").unwrap();
            headers.append("key1", "value1").unwrap();
            headers.append("key2", "value2").unwrap();
            headers.append("key3", "\"value3\"").unwrap();
            headers
        });
        assert_eq!(part.inner.unwrap().headers, {
            let mut headers = Headers::new();
            headers.append("accept", "text/html").unwrap();
            headers.append("key1", "value1").unwrap();
            headers.append("key2", "value2").unwrap();
            headers.append("key3", "\"value3\"").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePartBuilder::insert_header`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder`.
    /// 2. Inserts header.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_builder_insert_header() {
        let part = MimePartBuilder::new()
            .header("accept", "text/html")
            .header("accept", "text/plain");
        assert_eq!(part.inner.unwrap().headers, {
            let mut headers = Headers::new();
            headers.append("accept", "text/plain").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePartBuilder::append_header`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder`.
    /// 2. Appends header.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_builder_append_header() {
        let part = MimePartBuilder::new()
            .append_header("accept", "text/html")
            .append_header("accept", "text/plain");
        let binding = part.inner.unwrap();
        let value = binding.headers.get("accept");
        assert_eq!(value.unwrap().to_string().unwrap(), "text/html, text/plain");
    }

    /// UT test cases for `MimePart::builder`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets headers.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_builder() {
        let part = MimePart::builder()
            .header("key1", "value1")
            .build()
            .unwrap();
        assert_eq!(part.headers().to_owned(), {
            let mut headers = Headers::new();
            headers.insert("key1", "value1").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePart::headers`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets headers.
    /// 3. Gets the reference of headers then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_headers() {
        let part = MimePart::builder()
            .header("key1", "value1")
            .build()
            .unwrap();
        assert_eq!(part.headers().to_owned(), {
            let mut headers = Headers::new();
            headers.insert("key1", "value1").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePart::headers_mut`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets headers.
    /// 3. Gets the mutable reference of headers then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_headers_mut() {
        let part = MimePart::builder()
            .header("key1", "value1")
            .build()
            .unwrap();
        assert_eq!(part.headers().to_owned(), {
            let mut headers = Headers::new();
            headers.insert("key1", "value1").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePart::headers_once`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets headers.
    /// 3. Gets the ownership of headers then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_headers_once() {
        let mut part = MimePart::builder()
            .header("key1", "value1")
            .build()
            .unwrap();
        assert_eq!(part.headers_once(), {
            let mut headers = Headers::new();
            headers.insert("key1", "value1").unwrap();
            headers
        });
    }

    /// UT test cases for `MimePart::body`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets body.
    /// 3. Gets the reference of body then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_body() {
        let part = MimePart::builder()
            .body_from_owned(b"1111".to_vec())
            .build()
            .unwrap();
        assert_eq!(part.body().unwrap(), b"1111");
    }

    /// UT test cases for `MimePart::body_mut`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets body.
    /// 3. Gets the mutable reference of body then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_body_mut() {
        let mut part = MimePart::builder()
            .body_from_owned(b"1111".to_vec())
            .build()
            .unwrap();
        part.body_mut().unwrap()[0] = b'2';
        assert_eq!(part.body().unwrap(), b"2111");
    }

    /// UT test cases for `MimePart::body_once`.
    ///
    /// # Brief
    /// 1. Creates a `MimePartBuilder` by `MimePart::builder`.
    /// 2. Sets body.
    /// 3. Gets the ownership of body then compares.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_mime_part_body_once() {
        let mut part = MimePart::builder()
            .body_from_owned(b"1111".to_vec())
            .build()
            .unwrap();
        assert_eq!(part.body_once().unwrap(), b"1111".to_vec());
        assert_eq!(part.body().unwrap(), b"");
    }
}
