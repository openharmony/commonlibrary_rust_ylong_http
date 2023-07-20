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

use crate::body::mime::{DecodeHeaders, MimePart};
use crate::error::{ErrorKind, HttpError};
use crate::headers::{Header, HeaderName, HeaderValue, Headers};

/// `MimeMulti` is a Composite MIME body which is defined in [`RFC2046`]: \
/// In the case of multipart entities, in which one or more different
/// sets of data are combined in a single body, a "multipart" media type
/// field must appear in the entity's header.  The body must then contain
/// one or more body parts, each preceded by a boundary delimiter line,
/// and the last one followed by a closing boundary delimiter line. \
///
/// `MimeMulti` can be built by [`MimeMultiBuilder`], then can set headers and
/// boundary, and add part([`MimePart`] or `MimeMulti`).
///
/// [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1
/// [`MimeMultiBuilder`]: MimeMultiBuilder
/// [`MimePart`]: MimePart
///
/// # Examples
///
/// ```
/// use ylong_http::body::{MimeMulti, MimePart};
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
/// ```
#[derive(Debug, Default, PartialEq)]
pub struct MimeMulti<'a> {
    // [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1
    //
    // |========================================================================
    // |   dash-boundary := "--" boundary                                      |
    // |                    ; boundary taken from the value of                 |
    // |                    ; boundary parameter of the                        |
    // |                    ; Content-Type field.                              |
    // |                                                                       |
    // |   multipart-body := [preamble CRLF]                                   |
    // |                     dash-boundary transport-padding CRLF              |
    // |                     body-part *encapsulation                          |
    // |                     close-delimiter transport-padding                 |
    // |                     [CRLF epilogue]                                   |
    // |                                                                       |
    // |  transport-padding := *LWSP-char                                      |
    // |                        ; Composers MUST NOT generate                  |
    // |                        ; non-zero length transport                    |
    // |                        ; padding, but receivers MUST                  |
    // |                        ; be able to handle padding                    |
    // |                        ; added by message transports.                 |
    // |                                                                       |
    // |   encapsulation := delimiter transport-padding                        |
    // |                    CRLF body-part                                     |
    // |                                                                       |
    // |   delimiter := CRLF dash-boundary                                     |
    // |                                                                       |
    // |   close-delimiter := delimiter "--"                                   |
    // |                                                                       |
    // |   preamble := discard-text                                            |
    // |                                                                       |
    // |   epilogue := discard-text                                            |
    // |                                                                       |
    // |   discard-text := *(*text CRLF) *text                                 |
    // |                   ; May be ignored or discarded.                      |
    // |                                                                       |
    // |   body-part := MIME-part-headers [CRLF *OCTET]                        |
    // |                ; Lines in a body-part must not start                  |
    // |                ; with the specified dash-boundary and                 |
    // |                ; the delimiter must not appear anywhere               |
    // |                ; in the body part.  Note that the                     |
    // |                ; semantics of a body-part differ from                 |
    // |                ; the semantics of a message, as                       |
    // |                ; described in the text.                               |
    // |                                                                       |
    // |   OCTET := <any 0-255 octet value>                                    |
    // |========================================================================
    pub(crate) headers: Headers,
    pub(crate) boundary: Vec<u8>,
    pub(crate) list: Vec<XPart<'a>>,
}

impl<'a> MimeMulti<'a> {
    pub(crate) fn new() -> Self {
        MimeMulti {
            headers: Headers::new(),

            // In [`RFC 2046`]
            // The simplest boundary delimiter line possible is something like "---",
            // with a closing boundary delimiter line of "-----".
            // [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1.1
            boundary: b"-".to_vec(),
            list: vec![],
        }
    }

    pub(crate) fn set_headers(&mut self, headers: Headers) {
        self.headers = headers;
    }

    pub(crate) fn insert_header<N, V>(
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

    pub(crate) fn set_boundary(&mut self, boundary: Vec<u8>) {
        self.boundary = boundary;
    }

    pub(crate) fn set_content_type(
        &mut self,
        content_type: &[u8],
        boundary: Vec<u8>,
    ) -> Result<&mut Headers, HttpError> {
        let mut v = vec![];
        v.extend_from_slice(content_type);
        v.extend_from_slice(b"; boundary=");
        v.extend_from_slice(&boundary);

        self.boundary = boundary;
        self.headers.insert("Content-Type", &v[..])?;
        Ok(self.headers_mut())
    }

    pub(crate) fn add_part(&mut self, part: MimePart<'a>) {
        self.list.push(XPart::Part(part));
    }

    pub(crate) fn add_multi(&mut self, multi: MimeMulti<'a>) {
        self.list.push(XPart::Multi(multi));
    }

    pub(crate) fn add_xpart(&mut self, xpart: XPart<'a>) {
        self.list.push(xpart);
    }

    /// Creates a [`MimeMultiBuilder`].
    ///
    /// [`MimeMultiBuilder`]: MimeMultiBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMulti;
    ///
    /// let multi = MimeMulti::builder().build().unwrap();
    /// ```
    pub fn builder() -> MimeMultiBuilder<'a> {
        MimeMultiBuilder::new()
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

    /// Gets the reference of boundary.
    pub fn boundary(&self) -> &[u8] {
        &self.boundary
    }

    /// Gets the mutable reference of boundary.
    pub fn boundary_mut(&mut self) -> &mut [u8] {
        &mut self.boundary
    }

    /// Gets the ownership of boundary by replacing it with an default value.
    pub fn boundary_once(&mut self) -> Vec<u8> {
        take(&mut self.boundary)
    }

    /// Gets the reference of part list.
    pub fn list(&self) -> &Vec<XPart> {
        &self.list
    }

    /// Gets the mutable reference of part list.
    pub fn list_mut(&mut self) -> &mut Vec<XPart<'a>> {
        &mut self.list
    }

    /// Gets the ownership of part list by replacing it with an default value.
    pub fn list_once(&mut self) -> Vec<XPart> {
        take(&mut self.list)
    }
}

/// `MimeMultiBuilder` can set set a Composite MIME body [`MimeMulti`]. \
/// `MimeMultiBuilder` can set headers and boundary, and add part([`MimePart`]
/// or [`MimeMulti`]).
///
/// [`MimePart`]: MimePart
/// [`MimeMulti`]: MimeMulti
///
/// # Examples
///
/// ```
/// use ylong_http::body::{MimeMulti, MimePart};
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
/// ```
pub struct MimeMultiBuilder<'a> {
    inner: Result<MimeMulti<'a>, HttpError>,
}

impl<'a> MimeMultiBuilder<'a> {
    /// Creates a new [`MimeMultiBuilder`].
    ///
    /// [`MimeMultiBuilder`]: MimeMultiBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    ///
    /// let mut multi = MimeMultiBuilder::new().build().unwrap();
    /// ```
    pub fn new() -> Self {
        MimeMultiBuilder {
            inner: Ok(MimeMulti::new()),
        }
    }

    /// Sets headers to the Composite MIME body. \
    /// It is recommended to use [`set_content_type`] to set header
    /// 'Content-Type' and set boundary simultaneously.
    ///
    /// [`set_content_type`]: MimeMultiBuilder::set_content_type
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    /// use ylong_http::headers::Headers;
    ///
    /// let multi = MimeMultiBuilder::new()
    ///     .set_headers({
    ///         let mut headers = Headers::new();
    ///         headers
    ///             .append("Content-Type", "multipart/mixed; boundary=ab")
    ///             .unwrap();
    ///         headers
    ///     })
    ///     .set_boundary(b"ab".to_vec());
    /// ```
    pub fn set_headers(mut self, headers: Headers) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.set_headers(headers);
            inner
        });
        self
    }

    /// Inserts header to the Composite MIME body.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    ///
    /// let multi = MimeMultiBuilder::new()
    ///     .insert_header("accept", "text/html")
    ///     .insert_header("accept", "text/plain");
    /// ```
    pub fn insert_header<N, V>(mut self, name: N, value: V) -> Self
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.inner = self.inner.and_then(move |mut inner| {
            inner.insert_header(name, value)?;
            Ok(inner)
        });
        self
    }

    /// Appends header to the Composite MIME body.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    ///
    /// let multi = MimeMultiBuilder::new()
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

    // TODO: boundary check
    /// The boundary is defined in [`RFC2046`].
    /// The only mandatory global parameter for the "multipart" media type is
    /// the boundary parameter, which consists of 1 to 70 characters from a
    /// set of characters known to be very robust through mail gateways, and
    /// NOT ending with white space. \
    /// It is recommended to use [`set_content_type`] to set header
    /// 'Content-Type' and set boundary simultaneously.
    ///
    /// [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1.1
    /// [`set_content_type`]: MimeMultiBuilder::set_content_type
    pub fn set_boundary(mut self, boundary: Vec<u8>) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.set_boundary(boundary);
            inner
        });
        self
    }

    /// It is recommended to use `set_content_type` to set header 'Content-Type'
    /// and set boundary simultaneously. \
    /// The Content-Type field for multipart entities requires one parameter,
    /// "boundary". A typical "multipart" Content-Type header field might look
    /// like this In [`RFC2046`]: \
    /// *Content-Type: multipart/mixed; boundary=gc0p4Jq0M2Yt08j34c0p*
    ///
    /// [`RFC2046`]: https://www.rfc-editor.org/rfc/rfc2046#section-5.1.1
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    ///
    /// let mut multi = MimeMultiBuilder::new().set_content_type(b"multipart/mixed", b"ab".to_vec());
    /// ```
    pub fn set_content_type(mut self, content_type: &'a [u8], boundary: Vec<u8>) -> Self {
        self.inner = self.inner.and_then(move |mut inner| {
            inner.set_content_type(content_type, boundary)?;
            Ok(inner)
        });
        self
    }

    /// Adds a single body part When it is a `MimePart`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimeMultiBuilder, MimePart};
    ///
    /// let part = MimePart::builder()
    ///     .header("key1", "value1")
    ///     .body_from_reader("9876543210\r\n".as_bytes())
    ///     .build()
    ///     .unwrap();
    /// let multi = MimeMultiBuilder::new().add_part(part);
    /// ```
    pub fn add_part(mut self, part: MimePart<'a>) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.add_part(part);
            inner
        });
        self
    }

    /// Adds a multi body part When it is a `MimeMulti`.
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimeMulti, MimeMultiBuilder};
    ///
    /// let multi1 = MimeMulti::builder().build().unwrap();
    /// let multi = MimeMultiBuilder::new().add_multi(multi1);
    /// ```
    pub fn add_multi(mut self, multi: MimeMulti<'a>) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.add_multi(multi);
            inner
        });
        self
    }

    /// Adds a multi body part When it is a `XPart`.
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{MimeMulti, MimeMultiBuilder, XPart};
    ///
    /// let multi1 = MimeMulti::builder().build().unwrap();
    /// let multi = MimeMultiBuilder::new().add_xpart(XPart::Multi(multi1));
    /// ```
    pub fn add_xpart(mut self, xpart: XPart<'a>) -> Self {
        self.inner = self.inner.map(move |mut inner| {
            inner.add_xpart(xpart);
            inner
        });
        self
    }

    /// Builds a [`MimeMulti`].
    ///
    /// [`MimeMulti`]: MimeMulti
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::MimeMultiBuilder;
    ///
    /// let multi = MimeMultiBuilder::new().build().unwrap();
    /// ```
    pub fn build(self) -> Result<MimeMulti<'a>, HttpError> {
        self.inner
    }
}

impl<'a> Default for MimeMultiBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// `MimePart` or `MimeMulti` as a part.
#[derive(Debug, PartialEq)]
pub enum XPart<'a> {
    /// `MimePart`
    Part(MimePart<'a>),
    /// `MimeMulti`
    Multi(MimeMulti<'a>),
}
