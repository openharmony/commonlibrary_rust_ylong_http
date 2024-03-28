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

//! HTTP [`Header`][header], which is called `Field` in [`RFC9110`].
//!
//! The module provides [`Header`], [`HeaderName`], [`HeaderValue`], [`Headers`]
//! and a number of types used for interacting with `Headers`.
//!
//! These types allow representing both `HTTP/1` and `HTTP/2` headers.
//!
//! [header]: https://httpwg.org/specs/rfc9110.html#fields
//! [`RFC9110`]: https://httpwg.org/specs/rfc9110.html
//! [`Header`]: Header
//! [`HeaderName`]: HeaderName
//! [`HeaderValue`]: HeaderValue
//! [`Headers`]: Headers
//!
//! # Examples
//!
//! ```
//! use ylong_http::headers::Headers;
//!
//! let mut headers = Headers::new();
//! headers.insert("Accept", "text/html").unwrap();
//! headers.insert("Content-Length", "3495").unwrap();
//!
//! assert_eq!(
//!     headers.get("accept").unwrap().to_string().unwrap(),
//!     "text/html"
//! );
//! assert_eq!(
//!     headers.get("content-length").unwrap().to_string().unwrap(),
//!     "3495"
//! );
//! ```

use core::convert::TryFrom;
use core::{fmt, slice, str};
use std::collections::hash_map::Entry;
use std::collections::{hash_map, HashMap};

use crate::error::{ErrorKind, HttpError};

/// HTTP `Header`, which consists of [`HeaderName`] and [`HeaderValue`].
///
/// `Header` is called `Field` in RFC9110. HTTP uses fields to provide data in
/// the form of extensible name/value pairs with a registered key namespace.
///
/// [`HeaderName`]: HeaderName
/// [`HeaderValue`]: HeaderValue
///
/// # Examples
///
/// ```
/// use core::convert::TryFrom;
///
/// use ylong_http::headers::Header;
///
/// // This header name string will be normalized to lowercase.
/// let header = Header::try_from(("Example-Field", "Foo")).unwrap();
/// assert_eq!(header.name().as_bytes(), b"example-field");
///
/// // All characters of this header string can be displayed, so the `to_string`
/// // interface can be used to output.
/// assert_eq!(header.value().to_string().unwrap(), "Foo");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    name: HeaderName,
    value: HeaderValue,
}

impl Header {
    /// Combines a `HeaderName` and a `HeaderValue` into a `Header`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::{Header, HeaderName, HeaderValue};
    ///
    /// let name = HeaderName::from_bytes(b"Example-Field").unwrap();
    /// let value = HeaderValue::from_bytes(b"Foo").unwrap();
    ///
    /// let header = Header::from_raw_parts(name, value);
    /// assert_eq!(header.name().as_bytes(), b"example-field");
    /// assert_eq!(header.value().to_string().unwrap(), "Foo");
    /// ```
    pub fn from_raw_parts(name: HeaderName, value: HeaderValue) -> Self {
        Self { name, value }
    }

    /// Gets a reference to the underlying `HeaderName`.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::convert::TryFrom;
    ///
    /// use ylong_http::headers::Header;
    ///
    /// let header = Header::try_from(("Example-Field", "Foo")).unwrap();
    ///
    /// let name = header.name();
    /// assert_eq!(name.as_bytes(), b"example-field");
    /// ```
    pub fn name(&self) -> &HeaderName {
        &self.name
    }

    /// Gets a reference to the underlying `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::convert::TryFrom;
    ///
    /// use ylong_http::headers::Header;
    ///
    /// let header = Header::try_from(("Example-Field", "Foo")).unwrap();
    ///
    /// let value = header.value();
    /// assert_eq!(value.to_string().unwrap(), "Foo");
    /// ```
    pub fn value(&self) -> &HeaderValue {
        &self.value
    }

    /// Consumes this `Header`, get the underlying `HeaderName` and
    /// `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::convert::TryFrom;
    ///
    /// use ylong_http::headers::Header;
    ///
    /// let header = Header::try_from(("Example-Field", "Foo")).unwrap();
    /// let (name, value) = header.into_parts();
    ///
    /// assert_eq!(name.as_bytes(), b"example-field");
    /// assert_eq!(value.to_string().unwrap(), "Foo");
    /// ```
    pub fn into_parts(self) -> (HeaderName, HeaderValue) {
        (self.name, self.value)
    }
}

impl<N, V> TryFrom<(N, V)> for Header
where
    HeaderName: TryFrom<N>,
    <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
    HeaderValue: TryFrom<V>,
    <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
{
    type Error = HttpError;

    fn try_from(pair: (N, V)) -> Result<Self, Self::Error> {
        Ok(Self::from_raw_parts(
            HeaderName::try_from(pair.0).map_err(Into::into)?,
            HeaderValue::try_from(pair.1).map_err(Into::into)?,
        ))
    }
}

/// HTTP `Header Name`, which is called [`Field Name`] in RFC9110.
///
/// A field name labels the corresponding field value as having the semantics
/// defined by that name.
///
/// [`Field Name`]: https://httpwg.org/specs/rfc9110.html#fields.names
///
/// # Examples
///
/// ```
/// use ylong_http::headers::HeaderName;
///
/// let name = HeaderName::from_bytes(b"Example-Field").unwrap();
/// assert_eq!(name.as_bytes(), b"example-field");
/// ```
// TODO: `StandardHeader` implementation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HeaderName {
    name: String,
}

impl HeaderName {
    /// Converts a slice of bytes to a `HeaderName`.
    ///
    /// Since `HeaderName` is case-insensitive, characters of the input will be
    /// checked and then converted to lowercase.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderName;
    ///
    /// let name = HeaderName::from_bytes(b"Example-Field").unwrap();
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HttpError> {
        Ok(Self {
            name: Self::normalize(bytes)?,
        })
    }

    /// Returns a bytes representation of the `HeaderName`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderName;
    ///
    /// let name = HeaderName::from_bytes(b"Example-Field").unwrap();
    /// let bytes = name.as_bytes();
    /// assert_eq!(bytes, b"example-field");
    /// ```
    pub fn as_bytes(&self) -> &[u8] {
        self.name.as_bytes()
    }

    // Returns a Vec<u8> of the `HeaderName`.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.name.into_bytes()
    }

    /// Normalizes the input bytes.
    fn normalize(input: &[u8]) -> Result<String, HttpError> {
        let mut dst = Vec::new();
        for b in input.iter() {
            // HEADER_CHARS maps all bytes to valid single-byte UTF-8.
            let b = HEADER_CHARS[*b as usize];
            if b == 0 {
                return Err(ErrorKind::InvalidInput.into());
            }
            dst.push(b);
        }
        Ok(unsafe { String::from_utf8_unchecked(dst) })
    }
}

/// Returns a `String` value of the `HeaderName`.
///
/// # Examples
///
/// ```
/// use ylong_http::headers::HeaderName;
///
/// let name = HeaderName::from_bytes(b"Example-Field").unwrap();
/// let name_str = name.to_string();
/// assert_eq!(name_str, "example-field");
/// ```
impl ToString for HeaderName {
    fn to_string(&self) -> String {
        self.name.clone()
    }
}

impl TryFrom<&str> for HeaderName {
    type Error = HttpError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::from_bytes(name.as_bytes())
    }
}

impl TryFrom<&[u8]> for HeaderName {
    type Error = HttpError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

/// HTTP `Header Value`, which is called [`Field Value`] in RFC9110.
///
/// HTTP field values consist of a sequence of characters in a format defined by
/// the field's grammar.
///
/// [`Field Value`]: https://httpwg.org/specs/rfc9110.html#fields.values
///
/// # Examples
///
/// ```
/// use ylong_http::headers::HeaderValue;
///
/// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
/// value.append_bytes(b"application/xml").unwrap();
///
/// assert_eq!(value.to_string().unwrap(), "text/html, application/xml");
/// assert!(!value.is_sensitive());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderValue {
    inner: Vec<Vec<u8>>,
    // sensitive data: password etc.
    is_sensitive: bool,
}

impl HeaderValue {
    /// Attempts to convert a byte slice to a non-sensitive `HeaderValue`.
    ///
    /// `HeaderValue` is case-sensitive. Legal characters will remain unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// assert_eq!(value.to_string().unwrap(), "text/html");
    /// assert!(!value.is_sensitive());
    ///
    /// // `HeaderValue` is case-sensitive. Legal characters will remain unchanged.
    /// let value = HeaderValue::from_bytes(b"TEXT/HTML").unwrap();
    /// assert_eq!(value.to_string().unwrap(), "TEXT/HTML");
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HttpError> {
        if !bytes.iter().all(|b| Self::is_valid(*b)) {
            return Err(ErrorKind::InvalidInput.into());
        }

        Ok(HeaderValue {
            inner: vec![bytes.to_vec()],
            is_sensitive: false,
        })
    }

    /// Consume another `HeaderValue`, and then appends it to this
    /// `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// let other = HeaderValue::from_bytes(b"text/plain").unwrap();
    ///
    /// value.append(other);
    /// assert_eq!(value.to_string().unwrap(), "text/html, text/plain");
    /// ```
    pub fn append(&mut self, mut other: Self) {
        self.inner.append(&mut other.inner)
    }

    /// Appends a new bytes to `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// value.append_bytes(b"application/xml").unwrap();
    ///
    /// assert_eq!(value.to_string().unwrap(), "text/html, application/xml");
    /// ```
    pub fn append_bytes(&mut self, bytes: &[u8]) -> Result<(), HttpError> {
        if !bytes.iter().all(|b| Self::is_valid(*b)) {
            return Err(ErrorKind::InvalidInput.into());
        }
        self.inner.push(bytes.to_vec());
        Ok(())
    }

    /// Outputs the content of value as a string in a certain way.
    ///
    /// If there are characters that cannot be displayed in value, return `Err`.
    /// Extra comma and whitespace(", ") will be added between each element of
    /// value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// value.append_bytes(b"application/xml").unwrap();
    ///
    /// assert_eq!(value.to_string().unwrap(), "text/html, application/xml");
    /// ```
    pub fn to_string(&self) -> Result<String, HttpError> {
        let mut content = Vec::new();
        for (n, i) in self.inner.iter().enumerate() {
            if n != 0 {
                content.extend_from_slice(b", ");
            }
            content.extend_from_slice(i.as_slice());
        }
        Ok(unsafe { String::from_utf8_unchecked(content) })
    }

    /// Outputs the content of value as a Vec<u8> in a certain way.
    pub(crate) fn to_vec(&self) -> Vec<u8> {
        let mut content = Vec::new();
        for (n, i) in self.inner.iter().enumerate() {
            if n != 0 {
                content.extend_from_slice(b", ");
            }
            content.extend_from_slice(i.as_slice());
        }
        content
    }

    /// Returns an iterator over the `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// value.append_bytes(b"application/xml").unwrap();
    ///
    /// for sub_value in value.iter() {
    ///     // Operate on each sub-value.
    /// }
    /// ```
    pub fn iter(&self) -> HeaderValueIter<'_> {
        self.inner.iter()
    }

    /// Returns an iterator that allows modifying each sub-value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// value.append_bytes(b"application/xml").unwrap();
    ///
    /// for sub_value in value.iter_mut() {
    ///     // Operate on each sub-value.
    /// }
    /// ```
    pub fn iter_mut(&mut self) -> HeaderValueIterMut<'_> {
        self.inner.iter_mut()
    }

    /// Sets the sensitivity of value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// assert!(!value.is_sensitive());
    ///
    /// value.set_sensitive(true);
    /// assert!(value.is_sensitive());
    /// ```
    pub fn set_sensitive(&mut self, is_sensitive: bool) {
        self.is_sensitive = is_sensitive;
    }

    /// Returns `true` if the value represents sensitive data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::HeaderValue;
    ///
    /// let value = HeaderValue::from_bytes(b"text/html").unwrap();
    /// assert!(!value.is_sensitive());
    /// ```
    pub fn is_sensitive(&self) -> bool {
        self.is_sensitive
    }

    /// Returns `true` if the character matches the rules of `HeaderValue`.
    fn is_valid(b: u8) -> bool {
        b >= 32 && b != 127 || b == b'\t'
    }
}

impl TryFrom<&str> for HeaderValue {
    type Error = HttpError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_bytes(value.as_bytes())
    }
}

// `HeaderValue` can use `%x80-FF` u8 in [`RFC9110`].
// [`RFC9110`]: https://www.rfc-editor.org/rfc/rfc9110.html#name-field-values
//
// |========================================================================
// |   field-value    = *field-content                                     |
// |   field-content  = field-vchar                                        |
// |                    [ 1*( SP / HTAB / field-vchar ) field-vchar ]      |
// |   field-vchar    = VCHAR / obs-text                                   |
// |   obs-text       = %x80-FF                                            |
// |========================================================================
impl TryFrom<&[u8]> for HeaderValue {
    type Error = HttpError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

/// Immutable `HeaderValue` iterator.
///
/// This struct is created by [`HeaderValue::iter`].
///
/// [`HeaderValue::iter`]: HeaderValue::iter
///
/// # Examples
///
/// ```
/// use ylong_http::headers::HeaderValue;
///
/// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
/// value.append_bytes(b"application/xml").unwrap();
///
/// for sub_value in value.iter() {
///     // Operate on each sub-value.
/// }
/// ```
pub type HeaderValueIter<'a> = slice::Iter<'a, Vec<u8>>;

/// Mutable `HeaderValue` iterator.
///
/// This struct is created by [`HeaderValue::iter_mut`].
///
/// [`HeaderValue::iter_mut`]: HeaderValue::iter_mut
///
/// # Examples
///
/// ```
/// use ylong_http::headers::HeaderValue;
///
/// let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
/// value.append_bytes(b"application/xml").unwrap();
///
/// for sub_value in value.iter_mut() {
///     // Operate on each sub-value.
/// }
/// ```
pub type HeaderValueIterMut<'a> = slice::IterMut<'a, Vec<u8>>;

/// HTTP `Headers`, which is called [`Fields`] in RFC9110.
///
/// Fields are sent and received within the header and trailer sections of
/// messages.
///
/// [`Fields`]: https://httpwg.org/specs/rfc9110.html#fields
///
/// # Examples
///
/// ```
/// use ylong_http::headers::Headers;
///
/// let mut headers = Headers::new();
/// headers.insert("Accept", "text/html").unwrap();
/// headers.insert("Content-Length", "3495").unwrap();
/// headers.append("Accept", "text/plain").unwrap();
///
/// assert_eq!(
///     headers.get("accept").unwrap().to_string().unwrap(),
///     "text/html, text/plain"
/// );
/// assert_eq!(
///     headers.get("content-length").unwrap().to_string().unwrap(),
///     "3495"
/// );
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Headers {
    map: HashMap<HeaderName, HeaderValue>,
}

impl fmt::Display for Headers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (k, v) in self.iter() {
            writeln!(
                f,
                "{}: {}",
                k.to_string(),
                v.to_string()
                    .unwrap_or_else(|_| "<non-visible header value>".to_string())
            )?;
        }
        Ok(())
    }
}

impl Headers {
    /// Creates a new, empty `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let headers = Headers::new();
    /// assert!(headers.is_empty());
    /// ```
    pub fn new() -> Self {
        Headers {
            map: HashMap::new(),
        }
    }

    /// Returns the number of header in the `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// assert_eq!(headers.len(), 0);
    ///
    /// headers.insert("accept", "text/html").unwrap();
    /// assert_eq!(headers.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the `Headers` contains no headers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// assert!(headers.is_empty());
    ///
    /// headers.insert("accept", "text/html").unwrap();
    /// assert!(!headers.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns an immutable reference to the `HeaderValue` corresponding to
    /// the `HeaderName`.
    ///
    /// This method returns `None` if the input argument could not be
    /// successfully converted to a `HeaderName` or the `HeaderName` is not in
    /// `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// let value = headers.get("accept");
    /// assert_eq!(value.unwrap().to_string().unwrap(), "text/html");
    /// ```
    pub fn get<T>(&self, name: T) -> Option<&HeaderValue>
    where
        HeaderName: TryFrom<T>,
    {
        HeaderName::try_from(name)
            .ok()
            .and_then(|name| self.map.get(&name))
    }

    /// Returns a mutable reference to the `HeaderValue` corresponding to
    /// the `HeaderName`.
    ///
    /// This method returns `None` if the input argument could not be
    /// successfully converted to a `HeaderName` or the `HeaderName` is not in
    /// `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// let value = headers.get_mut("accept");
    /// assert_eq!(value.unwrap().to_string().unwrap(), "text/html");
    /// ```
    pub fn get_mut<T>(&mut self, name: T) -> Option<&mut HeaderValue>
    where
        HeaderName: TryFrom<T>,
    {
        HeaderName::try_from(name)
            .ok()
            .and_then(move |name| self.map.get_mut(&name))
    }

    /// Inserts a `Header` into the `Headers`.
    ///
    /// If the input argument could not be successfully converted to a `Header`,
    /// `Err` is returned.
    ///
    /// If the `Headers` did not have this `HeaderName` present, `None` is
    /// returned.
    ///
    /// If the `Headers` did have this `HeaderName` present, the new
    /// `HeaderValue` is updated, and the old `HeaderValue` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// assert!(headers.insert("\0", "illegal header").is_err());
    ///
    /// assert_eq!(headers.insert("accept", "text/html"), Ok(None));
    ///
    /// let old_value = headers.insert("accept", "text/plain").unwrap();
    /// assert_eq!(old_value.unwrap().to_string().unwrap(), "text/html");
    /// ```
    pub fn insert<N, V>(&mut self, name: N, value: V) -> Result<Option<HeaderValue>, HttpError>
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        let name = HeaderName::try_from(name).map_err(Into::into)?;
        let value = HeaderValue::try_from(value).map_err(Into::into)?;
        Ok(self.map.insert(name, value))
    }

    /// Appends a `Header` to the `Headers`.
    ///
    /// If the input argument could not be successfully converted to a `Header`,
    /// `Err` is returned.
    ///
    /// If the `Headers` did not have this `HeaderName` present, this `Header`
    /// is inserted into the `Headers`.
    ///
    /// If the `Headers` did have this `HeaderName` present, the new
    /// `HeaderValue` is appended to the old `HeaderValue`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// assert!(headers.append("\0", "illegal header").is_err());
    ///
    /// headers.append("accept", "text/html").unwrap();
    /// headers.append("accept", "text/plain").unwrap();
    ///
    /// let value = headers.get("accept");
    /// assert_eq!(value.unwrap().to_string().unwrap(), "text/html, text/plain");
    /// ```
    pub fn append<N, V>(&mut self, name: N, value: V) -> Result<(), HttpError>
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        let name = HeaderName::try_from(name).map_err(Into::into)?;
        let value = HeaderValue::try_from(value).map_err(Into::into)?;

        match self.map.entry(name) {
            Entry::Occupied(o) => {
                o.into_mut().append(value);
            }
            Entry::Vacant(v) => {
                let _ = v.insert(value);
            }
        };
        Ok(())
    }

    /// Removes `Header` from `Headers` by `HeaderName`, returning the
    /// `HeaderValue` at the `HeaderName` if the `HeaderName` was previously
    /// in the `Headers`.
    ///
    /// If the input argument could not be successfully converted to a `Header`,
    /// `None` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// let value = headers.remove("accept");
    /// assert_eq!(value.unwrap().to_string().unwrap(), "text/html");
    /// ```
    pub fn remove<T>(&mut self, name: T) -> Option<HeaderValue>
    where
        HeaderName: TryFrom<T>,
    {
        HeaderName::try_from(name)
            .ok()
            .and_then(|name| self.map.remove(&name))
    }

    /// Returns an iterator over the `Headers`. The iterator element type is
    /// `(&'a HeaderName, &'a HeaderValue)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// for (_name, _value) in headers.iter() {
    ///     // Operate on each `HeaderName` and `HeaderValue` pair.
    /// }
    /// ```
    pub fn iter(&self) -> HeadersIter<'_> {
        self.map.iter()
    }

    /// Returns an iterator over the `Headers`. The iterator element type is
    /// `(&'a HeaderName, &'a mut HeaderValue)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// for (_name, _value) in headers.iter_mut() {
    ///     // Operate on each `HeaderName` and `HeaderValue` pair.
    /// }
    /// ```
    pub fn iter_mut(&mut self) -> HeadersIterMut<'_> {
        self.map.iter_mut()
    }
}

impl IntoIterator for Headers {
    type Item = (HeaderName, HeaderValue);
    type IntoIter = HeadersIntoIter;

    /// Creates a consuming iterator, that is, one that moves each `HeaderName`
    /// and `HeaderValue` pair out of the `Headers` in arbitrary order. The
    /// `Headers` cannot be used after calling this.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// headers.append("accept", "text/html").unwrap();
    ///
    /// for (_name, _value) in headers.into_iter() {
    ///     // Operate on each `HeaderName` and `HeaderValue` pair.
    /// }
    /// ```
    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a Headers {
    type Item = (&'a HeaderName, &'a HeaderValue);
    type IntoIter = HeadersIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Headers {
    type Item = (&'a HeaderName, &'a mut HeaderValue);
    type IntoIter = HeadersIterMut<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Immutable `Headers` iterator.
///
/// This struct is created by [`Headers::iter`].
///
/// [`Headers::iter`]: Headers::iter
///
/// # Examples
///
/// ```
/// use ylong_http::headers::Headers;
///
/// let mut headers = Headers::new();
/// headers.append("accept", "text/html").unwrap();
///
/// for (_name, _value) in headers.iter() {
///     // Operate on each `HeaderName` and `HeaderValue` pair.
/// }
/// ```
pub type HeadersIter<'a> = hash_map::Iter<'a, HeaderName, HeaderValue>;

/// Mutable `Headers` iterator.
///
/// This struct is created by [`Headers::iter_mut`].
///
/// [`Headers::iter_mut`]: Headers::iter_mut
///
/// # Examples
///
/// ```
/// use ylong_http::headers::Headers;
///
/// let mut headers = Headers::new();
/// headers.append("accept", "text/html").unwrap();
///
/// for (_name, _value) in headers.iter_mut() {
///     // Operate on each `HeaderName` and `HeaderValue` pair.
/// }
/// ```
pub type HeadersIterMut<'a> = hash_map::IterMut<'a, HeaderName, HeaderValue>;

/// An owning iterator over the entries of a `Headers`.
///
/// This struct is created by [`Headers::into_iter`].
///
/// [`Headers::into_iter`]: crate::headers::Headers::into_iter
///
/// # Examples
///
/// ```
/// use ylong_http::headers::Headers;
///
/// let mut headers = Headers::new();
/// headers.append("accept", "text/html").unwrap();
///
/// for (_name, _value) in headers.into_iter() {
///     // Operate on each `HeaderName` and `HeaderValue` pair.
/// }
/// ```
pub type HeadersIntoIter = hash_map::IntoIter<HeaderName, HeaderValue>;

// HEADER_CHARS is used to check whether char is correct and transfer to
// lowercase.
#[rustfmt::skip]
const HEADER_CHARS: [u8; 256] = [
//  0       1       2       3       4       5       6       7       8       9
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 0x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 1x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 2x
    0,      0,      b' ',   b'!',   b'"',   b'#',   b'$',   b'%',   b'&',   b'\'',  // 3x
    0,      0,      b'*',   b'+',   b',',   b'-',   b'.',   b'/',   b'0',   b'1',   // 4x
    b'2',   b'3',   b'4',   b'5',   b'6',   b'7',   b'8',   b'9',   0,      0,      // 5x
    0,      0,      0,      0,      0,      b'a',   b'b',   b'c',   b'd',   b'e',   // 6x
    b'f',   b'g',   b'h',   b'i',   b'j',   b'k',   b'l',   b'm',   b'n',   b'o',   // 7x
    b'p',   b'q',   b'r',   b's',   b't',   b'u',   b'v',   b'w',   b'x',   b'y',   // 8x
    b'z',   0,      0,      0,      b'^',   b'_',   b'`',   b'a',   b'b',   b'c',   // 9x
    b'd',   b'e',   b'f',   b'g',   b'h',   b'i',   b'j',   b'k',   b'l',   b'm',   // 10x
    b'n',   b'o',   b'p',   b'q',   b'r',   b's',   b't',   b'u',   b'v',   b'w',   // 11x
    b'x',   b'y',   b'z',   0,      b'|',   0,      b'~',   0,      0,      0,      // 12x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 13x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 14x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 15x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 16x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 17x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 18x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 19x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 20x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 21x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 22x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 23x
    0,      0,      0,      0,      0,      0,      0,      0,      0,      0,      // 24x
    0,      0,      0,      0,      0,      0,                                      // 25x
];

#[cfg(test)]
mod ut_headers {
    use std::collections::HashMap;

    use crate::headers::{Header, HeaderName, HeaderValue, Headers};

    /// UT test cases for `HeaderName::from_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderName` by calling `HeaderName::from_bytes`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_header_name_from_bytes() {
        let name = String::from("accept");
        assert_eq!(
            HeaderName::from_bytes(b"ACCEPT"),
            Ok(HeaderName { name: name.clone() })
        );
        assert_eq!(HeaderName::from_bytes(b"accept"), Ok(HeaderName { name }));
    }

    /// UT test cases for `HeaderName::as_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderName`.
    /// 2. Fetches content from `HeaderName` by calling `HeaderName::as_bytes`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_header_name_as_bytes() {
        let name = HeaderName {
            name: "accept".to_string(),
        };
        assert_eq!(name.as_bytes(), b"accept");
    }

    /// UT test cases for `HeaderValue::from_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue` by calling `HeaderValue::from_bytes`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_header_value_from_bytes() {
        let value = HeaderValue::from_bytes(b"teXt/hTml, APPLICATION/xhtml+xml, application/xml");
        let result = Ok(HeaderValue {
            inner: vec![b"teXt/hTml, APPLICATION/xhtml+xml, application/xml".to_vec()],
            is_sensitive: false,
        });
        assert_eq!(value, result);
    }

    /// UT test cases for `Header::from_raw_parts, name, value, into_parts`.
    ///
    /// # Brief
    /// 1. Creates a `Header`.
    /// 2. Calls Header::from_raw_parts, name, value and into_parts
    ///    respectively.
    /// 3. Checks if the test results are corrent.
    #[test]
    fn ut_header_methods() {
        // from_raw_parts
        let name = HeaderName::from_bytes(b"John-Doe").unwrap();
        let value = HeaderValue::from_bytes(b"Foo").unwrap();
        let header = Header::from_raw_parts(name, value);
        assert_eq!(header.name().as_bytes(), b"john-doe");
        assert_eq!(header.value().to_string().unwrap(), "Foo");
        assert_ne!(header.name().as_bytes(), b"John-Doe");
        assert_ne!(header.value().to_string().unwrap(), "foo");

        // name
        let name = header.name();
        assert_eq!(name.as_bytes(), b"john-doe");
        assert_ne!(name.as_bytes(), b"John-Doe");
        assert_ne!(name.as_bytes(), b"jane-doe");

        // value
        let value = header.value();
        assert_eq!(value.to_string().unwrap(), "Foo");
        assert_ne!(value.to_string().unwrap(), "foo");
        assert_ne!(value.to_string().unwrap(), "oof");

        // into_parts
        let (name, value) = header.into_parts();
        assert_eq!(name.as_bytes(), b"john-doe");
        assert_eq!(value.to_string().unwrap(), "Foo");
        assert_ne!(name.as_bytes(), b"John-Doe");
        assert_ne!(value.to_string().unwrap(), "foo");
    }

    /// UT test cases for `HeaderValue::iter`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue`.
    /// 2. Loops through the values by calling `HeaderValue::iter`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_header_value_iter() {
        let mut value = HeaderValue::from_bytes(b"text/html").unwrap();
        value.append_bytes(b"application/xml").unwrap();
        let value_to_compare = ["text/html", "application/xml"];

        for (index, sub_value) in value.iter().enumerate() {
            assert_eq!(sub_value, value_to_compare[index].as_bytes());
        }

        for (index, sub_value) in value.iter_mut().enumerate() {
            assert_eq!(sub_value, value_to_compare[index].as_bytes());
        }
    }

    /// UT test cases for `HeaderValue::is_sensitive`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue`.
    /// 2. Calls `HeaderValue::is_sensitive` to check if the test results are
    ///    correct.
    #[test]
    fn ut_header_value_is_sensitive() {
        let mut value = HeaderValue {
            inner: vec![b"text/html, application/xhtml+xml".to_vec()],
            is_sensitive: true,
        };
        assert!(value.is_sensitive());
        value.is_sensitive = false;
        assert!(!value.is_sensitive());
    }

    /// UT test cases for `Headers::get_mut`.
    ///
    /// # Brief
    /// 1. Creates a `Headers`.
    /// 2. Gets the mutable `HeaderValue` by calling
    ///    `HeaderValue::append_bytes`.
    /// 3. Modifies `HeaderValue`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_headers_get_mut() {
        let mut headers = Headers::new();
        headers.append("accept", "text/css").unwrap();
        let value = headers.get_mut("accept").unwrap();
        assert!(!value.is_sensitive());
        value.is_sensitive = true;
        assert!(value.is_sensitive());
    }

    /// UT test cases for `HeaderValue::append_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue`.
    /// 2. Adds new value content into `HeaderValue` by calling
    ///    `HeaderValue::append_bytes`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_header_value_append_bytes() {
        let mut value = HeaderValue {
            inner: vec![b"text/html, application/xhtml+xml".to_vec()],
            is_sensitive: false,
        };
        assert!(value.append_bytes(b"teXt/plain, teXt/css").is_ok());
        assert!(value.append_bytes(b"application/xml").is_ok());

        let res = HeaderValue {
            inner: vec![
                b"text/html, application/xhtml+xml".to_vec(),
                b"teXt/plain, teXt/css".to_vec(),
                b"application/xml".to_vec(),
            ],
            is_sensitive: false,
        };
        assert_eq!(value, res);
    }

    /// UT test cases for `HeaderValue::to_string`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue`.
    /// 2. Gets content of `HeaderValue` by calling `HeaderName::to_string`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_header_value_to_string() {
        let value = HeaderValue {
            inner: vec![
                b"text/html, application/xhtml+xml".to_vec(),
                b"text/plain, text/css".to_vec(),
                b"application/xml".to_vec(),
            ],
            is_sensitive: false,
        };

        let result =
            "text/html, application/xhtml+xml, text/plain, text/css, application/xml".to_string();
        assert_eq!(value.to_string(), Ok(result));
    }

    /// UT test cases for `HeaderValue::set_sensitive`.
    ///
    /// # Brief
    /// 1. Creates a `HeaderValue`.
    /// 2. Sets content of `HeaderValue` by calling `HeaderName::set_sensitive`.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_header_value_set_sensitive() {
        let mut value = HeaderValue {
            inner: vec![],
            is_sensitive: false,
        };

        value.set_sensitive(true);
        assert!(value.is_sensitive);
    }

    /// UT test cases for `Headers::new`.
    ///
    /// # Brief
    /// 1. Creates `Headers` by calling `Headers::new`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_headers_new() {
        assert_eq!(
            Headers::new(),
            Headers {
                map: HashMap::new()
            }
        );
    }

    /// UT test cases for `ut_change_headers_info`.
    ///
    /// # Brief
    /// 1. Creates Headers
    /// 2. Adds content type `(&str, &str)` by calling append().
    /// 3. Uses new content to replace old content by calling insert().
    /// 4. Uses `HeaderName` to fetch `HeaderValue` by calling get().
    /// 5. Uses `HeaderNam`e to remove `HeaderValu`e by calling remove().
    /// 6. Checks if the test result is correct by assert_eq!().
    #[test]
    fn ut_change_headers_info() {
        let mut new_headers = Headers::new();
        if new_headers.is_empty() {
            let _ = new_headers.append("ACCEPT", "text/html");
        }
        let map_len = new_headers.len();
        assert_eq!(map_len, 1);

        let mut verify_map = HashMap::new();
        verify_map.insert(
            HeaderName {
                name: "accept".to_string(),
            },
            HeaderValue {
                inner: [b"text/html".to_vec()].to_vec(),
                is_sensitive: false,
            },
        );
        let headers_map = &new_headers.map;
        assert_eq!(headers_map, &verify_map);

        let mut value_vec = Vec::new();
        let inner_vec = b"text/html, application/xhtml+xml, application/xml".to_vec();
        value_vec.push(inner_vec);
        let _ = new_headers.insert(
            "accept",
            "text/html, application/xhtml+xml, application/xml",
        );

        let header_value = new_headers.get("accept").unwrap();
        let verify_value = HeaderValue {
            inner: value_vec,
            is_sensitive: false,
        };
        assert_eq!(header_value, &verify_value);

        let remove_value = new_headers.remove("accept").unwrap();
        assert_eq!(
            remove_value,
            HeaderValue {
                inner: [b"text/html, application/xhtml+xml, application/xml".to_vec()].to_vec(),
                is_sensitive: false
            }
        );
    }

    /// UT test cases for `Headers::iter`.
    ///
    /// # Brief
    /// 1. Creates a `Headers`.
    /// 2. Creates an iterator by calling `Headers::iter`.
    /// 3. Fetches `HeaderValue` content by calling `HeadersIter::next`.
    /// 4. Checks if the test results are correct.
    #[test]
    fn ut_headers_iter() {
        let mut headers = Headers::new();
        assert!(headers.append("ACCEPT", "text/html").is_ok());

        let mut iter = headers.iter();
        assert_eq!(
            iter.next(),
            Some((
                &HeaderName {
                    name: "accept".to_string()
                },
                &HeaderValue {
                    inner: [b"text/html".to_vec()].to_vec(),
                    is_sensitive: false
                }
            ))
        );
        assert_eq!(iter.next(), None);
    }
}
