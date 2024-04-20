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

//! HTTP [`URI`].
//!
//! URI references are used to target requests, indicate redirects, and define
//! relationships.
//!
//! [`URI`]: https://httpwg.org/specs/rfc9110.html#uri.references

mod percent_encoding;

use core::convert::{Infallible, TryFrom, TryInto};

pub use percent_encoding::PercentEncoder;

use crate::error::{ErrorKind, HttpError};

// Maximum uri length.
const MAX_URI_LEN: usize = (u16::MAX - 1) as usize;

/// HTTP [`URI`] implementation.
///
/// The complete structure of the uri is as follows:
///
/// ```text
/// | scheme://authority path ?query |
/// ```
///
/// `URI` currently only supports `HTTP` and `HTTPS` schemes.
///
/// According to [RFC9110, Section 4.2], the userinfo parameter before authority
/// is forbidden. Fragment information in query is not stored in uri.
///
/// So, the `URI` shown below is illegal:
///
/// ```text
/// http://username:password@example.com:80/
/// ```
///
/// [`URI`]: https://httpwg.org/specs/rfc9110.html#uri.references
/// [RFC9110, Section 4.2]: https://httpwg.org/specs/rfc9110.html#uri.schemes
///
/// # Examples
///
/// ```
/// use ylong_http::request::uri::Uri;
///
/// let uri = Uri::builder()
///     .scheme("http")
///     .authority("example.com:80")
///     .path("/foo")
///     .query("a=1")
///     .build()
///     .unwrap();
///
/// assert_eq!(uri.scheme().unwrap().as_str(), "http");
/// assert_eq!(uri.host().unwrap().as_str(), "example.com");
/// assert_eq!(uri.port().unwrap().as_str(), "80");
/// assert_eq!(uri.path().unwrap().as_str(), "/foo");
/// assert_eq!(uri.query().unwrap().as_str(), "a=1");
/// assert_eq!(uri.to_string(), "http://example.com:80/foo?a=1");
///
/// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
/// assert_eq!(uri.to_string(), "http://example.com:80/foo?a=1");
/// ```
#[derive(Clone, Debug, Default)]
pub struct Uri {
    /// The scheme can be `None` when the relative uri is used.
    scheme: Option<Scheme>,

    /// The authority can be `None` when the relative uri is used.
    authority: Option<Authority>,

    /// The path can be `None` when the path is "/".
    path: Option<Path>,

    /// The query can be `None` when the query is not set.
    query: Option<Query>,
}

impl Uri {
    /// Creates a HTTP-compliant default `Uri` with `Path` set to '/'.
    pub(crate) fn http() -> Uri {
        Uri {
            scheme: None,
            authority: None,
            path: Path::from_bytes(b"/").ok(),
            query: None,
        }
    }

    /// Creates a new default [`UriBuilder`].
    ///
    /// [`UriBuilder`]: UriBuilder
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let builder = Uri::builder();
    /// ```
    pub fn builder() -> UriBuilder {
        UriBuilder::new()
    }

    /// Gets a immutable reference to `Scheme`.
    ///
    /// Returns `None` if there is no `Scheme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.scheme().unwrap().as_str(), "http");
    /// ```
    pub fn scheme(&self) -> Option<&Scheme> {
        self.scheme.as_ref()
    }

    /// Gets a immutable reference to `Authority`.
    ///
    /// Returns `None` if there is no `Authority`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// let authority = uri.authority().unwrap();
    /// assert_eq!(authority.host().as_str(), "example.com");
    /// assert_eq!(authority.port().unwrap().as_str(), "80");
    /// ```
    pub fn authority(&self) -> Option<&Authority> {
        self.authority.as_ref()
    }

    /// Gets a immutable reference to `Host`.
    ///
    /// Returns `None` if there is no `Host`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.host().unwrap().as_str(), "example.com");
    /// ```
    pub fn host(&self) -> Option<&Host> {
        self.authority.as_ref().map(|auth| auth.host())
    }

    /// Gets a immutable reference to `Port`.
    ///
    /// Returns `None` if there is no `Port`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.port().unwrap().as_str(), "80");
    /// ```
    pub fn port(&self) -> Option<&Port> {
        self.authority.as_ref().and_then(|auth| auth.port())
    }

    /// Gets a immutable reference to `Path`.
    ///
    /// Returns `None` if there is no `Path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.path().unwrap().as_str(), "/foo");
    /// ```
    pub fn path(&self) -> Option<&Path> {
        self.path.as_ref()
    }

    /// Gets a immutable reference to `Query`.
    ///
    /// Returns `None` if there is no `Query`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.query().unwrap().as_str(), "a=1");
    /// ```
    pub fn query(&self) -> Option<&Query> {
        self.query.as_ref()
    }

    /// Converts a bytes slice into a `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"/foo?a=1").unwrap();
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HttpError> {
        if bytes.len() > MAX_URI_LEN {
            return Err(InvalidUri::UriTooLong.into());
        }
        if bytes.is_empty() {
            return Err(InvalidUri::InvalidFormat.into());
        }
        let (scheme, rest) = scheme_token(bytes)?;
        let (authority, rest) = authority_token(rest)?;
        let (path, rest) = path_token(rest)?;
        let query = query_token(rest)?;
        let result = Uri {
            scheme,
            authority,
            path,
            query,
        };
        validity_check(result).map_err(Into::into)
    }

    /// Gets a `String`, which contains the path and query.
    ///
    /// Returns `None` if path and query are both empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// assert_eq!(uri.path_and_query().unwrap(), String::from("/foo?a=1"));
    /// ```
    pub fn path_and_query(&self) -> Option<String> {
        let mut builder = String::new();
        if let Some(path) = self.path() {
            builder.push_str(path.as_str());
        }
        if let Some(query) = self.query() {
            builder.push('?');
            builder.push_str(query.as_str());
        }
        if builder.is_empty() {
            return None;
        }
        Some(builder)
    }

    /// Splits the `Uri` into its parts.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Scheme, Uri};
    ///
    /// let uri = Uri::from_bytes(b"http://example.com:80/foo?a=1").unwrap();
    /// let (scheme, auth, path, query) = uri.into_parts();
    /// assert_eq!(scheme, Some(Scheme::HTTP));
    /// assert_eq!(auth.unwrap().to_string(), String::from("example.com:80"));
    /// assert_eq!(path.unwrap().as_str(), "/foo");
    /// assert_eq!(query.unwrap().as_str(), "a=1");
    /// ```
    #[rustfmt::skip] // rust fmt check will add "," after `self`
    pub fn into_parts(
        self
    ) -> (
        Option<Scheme>,
        Option<Authority>,
        Option<Path>,
        Option<Query>,
    ) {
        (self.scheme, self.authority, self.path, self.query)
    }

    /// Creates an `Uri` from `Scheme`, `Authority`, `Path`, `Query`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Uri;
    ///
    /// let uri = Uri::from_raw_parts(None, None, None, None);
    /// ```
    pub fn from_raw_parts(
        scheme: Option<Scheme>,
        authority: Option<Authority>,
        path: Option<Path>,
        query: Option<Query>,
    ) -> Self {
        Self {
            scheme,
            authority,
            path,
            query,
        }
    }
}

impl ToString for Uri {
    fn to_string(&self) -> String {
        let mut builder = String::new();
        if let Some(scheme) = self.scheme() {
            builder.push_str(scheme.as_str());
            builder.push_str("://");
        }
        if let Some(host) = self.host() {
            builder.push_str(host.as_str());
        }
        if let Some(port) = self.port() {
            builder.push(':');
            builder.push_str(port.as_str());
        }
        if let Some(path) = self.path() {
            builder.push_str(path.as_str());
        }
        if let Some(query) = self.query() {
            builder.push('?');
            builder.push_str(query.as_str());
        }
        builder
    }
}

impl<'a> TryFrom<&'a [u8]> for Uri {
    type Error = HttpError;

    fn try_from(s: &'a [u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(s)
    }
}

impl<'a> TryFrom<&'a str> for Uri {
    type Error = HttpError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Self::from_bytes(s.as_bytes())
    }
}

/// A builder of `Uri`, which you can use it to construct a [`Uri`].
///
/// [`Uri`]: Uri
///
/// # Example
///
/// ```
/// use ylong_http::request::uri::Uri;
///
/// let uri = Uri::builder()
///     .scheme("http")
///     .authority("example.com:80")
///     .path("/foo")
///     .query("a=1")
///     .build()
///     .unwrap();
/// ```
pub struct UriBuilder {
    unprocessed: Result<Uri, InvalidUri>,
}

impl UriBuilder {
    /// Creates a new, default `UriBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// let uri = UriBuilder::new();
    /// ```
    pub fn new() -> Self {
        UriBuilder::default()
    }

    /// Sets the `Scheme` of `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// // This method takes a generic parameter that supports multiple types.
    /// let builder = UriBuilder::new().scheme("http");
    /// let builder = UriBuilder::new().scheme("http".as_bytes());
    /// ```
    pub fn scheme<T>(mut self, scheme: T) -> Self
    where
        Scheme: TryFrom<T>,
        InvalidUri: From<<Scheme as TryFrom<T>>::Error>,
    {
        self.unprocessed = self.unprocessed.and_then(move |mut unprocessed| {
            let scheme = scheme.try_into()?;
            unprocessed.scheme = Some(scheme);
            Ok(unprocessed)
        });
        self
    }

    /// Sets the `Authority` of `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// // This method takes a generic parameter that supports multiple types.
    /// let builder = UriBuilder::new().authority("example.com:80");
    /// let builder = UriBuilder::new().authority("example.com:80".as_bytes());
    /// ```
    pub fn authority<T>(mut self, auth: T) -> Self
    where
        Authority: TryFrom<T>,
        InvalidUri: From<<Authority as TryFrom<T>>::Error>,
    {
        self.unprocessed = self.unprocessed.and_then(move |mut unprocessed| {
            let auth = auth.try_into()?;
            unprocessed.authority = Some(auth);
            Ok(unprocessed)
        });
        self
    }

    /// Sets the `Path` of `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// let mut builder = UriBuilder::new().path("/foo");
    /// let mut builder = UriBuilder::new().path("/foo".as_bytes());
    /// ```
    pub fn path<T>(mut self, path: T) -> Self
    where
        Path: TryFrom<T>,
        InvalidUri: From<<Path as TryFrom<T>>::Error>,
    {
        self.unprocessed = self.unprocessed.and_then(move |mut unprocessed| {
            let path = path.try_into()?;
            unprocessed.path = Some(path);
            Ok(unprocessed)
        });
        self
    }

    /// Sets the `Query` of `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// let builder = UriBuilder::new().query("a=1");
    /// let builder = UriBuilder::new().query("a=1".as_bytes());
    /// ```
    pub fn query<T>(mut self, query: T) -> Self
    where
        Query: TryFrom<T>,
        InvalidUri: From<<Query as TryFrom<T>>::Error>,
    {
        self.unprocessed = self.unprocessed.and_then(move |mut unprocessed| {
            let query = query.try_into()?;
            unprocessed.query = Some(query);
            Ok(unprocessed)
        });
        self
    }

    /// Consumes the builder and constructs a valid `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::UriBuilder;
    ///
    /// let uri = UriBuilder::new()
    ///     .scheme("http")
    ///     .authority("example.com:80")
    ///     .path("/foo")
    ///     .query("a=1")
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn build(self) -> Result<Uri, HttpError> {
        self.unprocessed
            .and_then(validity_check)
            .map_err(Into::into)
    }
}

impl Default for UriBuilder {
    fn default() -> UriBuilder {
        UriBuilder {
            unprocessed: Ok(Uri {
                scheme: None,
                authority: None,
                path: None,
                query: None,
            }),
        }
    }
}

/// Scheme component of [`Uri`].
///
/// [`Uri`]: Uri
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Scheme {
    proto: Protocol,
}

impl Scheme {
    /// HTTP protocol Scheme.
    pub const HTTP: Scheme = Scheme {
        proto: Protocol::Http,
    };

    /// HTTPS protocol Scheme.
    pub const HTTPS: Scheme = Scheme {
        proto: Protocol::Https,
    };

    /// Converts a byte slice into a `Scheme`.
    ///
    /// This method only accepts `b"http"` and `b"https"` as input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Scheme;
    ///
    /// let scheme = Scheme::from_bytes(b"http").unwrap();
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Scheme, InvalidUri> {
        if bytes.eq_ignore_ascii_case(b"http") {
            Ok(Protocol::Http.into())
        } else if bytes.eq_ignore_ascii_case(b"https") {
            Ok(Protocol::Https.into())
        } else {
            Err(InvalidUri::InvalidScheme)
        }
    }

    /// Returns a string slice containing the entire `Scheme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Scheme;
    ///
    /// let scheme = Scheme::from_bytes(b"http").unwrap();
    /// assert_eq!(scheme.as_str(), "http");
    /// ```
    pub fn as_str(&self) -> &str {
        match &self.proto {
            Protocol::Http => "http",
            Protocol::Https => "https",
        }
    }

    /// Returns the default port of current uri `Scheme`.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_http::request::uri::Scheme;
    ///
    /// let scheme = Scheme::from_bytes(b"http").unwrap();
    /// assert_eq!(scheme.default_port(), 80);
    /// ```
    pub fn default_port(&self) -> u16 {
        match *self {
            Scheme::HTTP => 80,
            Scheme::HTTPS => 443,
        }
    }
}

impl From<Protocol> for Scheme {
    fn from(proto: Protocol) -> Self {
        Scheme { proto }
    }
}

impl<'a> TryFrom<&'a [u8]> for Scheme {
    type Error = InvalidUri;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        Scheme::from_bytes(bytes)
    }
}

impl<'a> TryFrom<&'a str> for Scheme {
    type Error = InvalidUri;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        TryFrom::try_from(s.as_bytes())
    }
}

/// Authority component of [`Uri`].
///
/// [`Uri`]: Uri
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Authority {
    host: Host,
    port: Option<Port>,
}

impl Authority {
    /// Converts a byte slice into a `Authority`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Authority;
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Authority, InvalidUri> {
        if bytes.is_empty() {
            return Err(InvalidUri::UriMissAuthority);
        }
        let (authority, rest) = authority_token(bytes)?;
        if !rest.is_empty() || authority.is_none() {
            return Err(InvalidUri::InvalidAuthority);
        }
        Ok(authority.unwrap())
    }

    /// Gets a immutable reference to `Host`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Authority, Host};
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let host = authority.host();
    /// assert_eq!(host.as_str(), "example.com");
    /// ```
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Gets a immutable reference to `Port`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Authority, Port};
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let port = authority.port().unwrap();
    /// assert_eq!(port.as_str(), "80");
    /// ```
    pub fn port(&self) -> Option<&Port> {
        self.port.as_ref()
    }

    /// Returns a string containing the entire `Authority`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Authority, Port};
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// assert_eq!(authority.to_str(), "example.com:80".to_string());
    /// ```
    pub fn to_str(&self) -> String {
        let mut auth = self.host.as_str().to_string();
        if let Some(ref p) = self.port {
            auth.push(':');
            auth.push_str(p.as_str());
        };
        auth
    }

    /// Splits the `Authority` into its parts.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Authority;
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let (host, port) = authority.into_parts();
    /// assert_eq!(host.as_str(), "example.com");
    /// assert_eq!(port.unwrap().as_u16().unwrap(), 80);
    /// ```
    pub fn into_parts(self) -> (Host, Option<Port>) {
        (self.host, self.port)
    }
}

impl<'a> TryFrom<&'a [u8]> for Authority {
    type Error = InvalidUri;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        Authority::from_bytes(bytes)
    }
}

impl<'a> TryFrom<&'a str> for Authority {
    type Error = InvalidUri;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        TryFrom::try_from(s.as_bytes())
    }
}

impl ToString for Authority {
    fn to_string(&self) -> String {
        let mut builder = String::new();
        builder.push_str(self.host().as_str());
        if let Some(port) = self.port() {
            builder.push(':');
            builder.push_str(port.as_str());
        }
        builder
    }
}

/// Host part of [`Authority`].
///
/// [`Authority`]: Authority
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Host(String);

impl Host {
    /// Returns a string slice containing the entire `Host`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Authority;
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let host = authority.host();
    /// assert_eq!(host.as_str(), "example.com");
    /// ```
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl ToString for Host {
    fn to_string(&self) -> String {
        self.0.to_owned()
    }
}

/// Port part of [`Authority`].
///
/// [`Authority`]: Authority
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Port(String);

impl Port {
    /// Returns a string slice containing the entire `Port`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Authority, Port};
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let port = authority.port().unwrap();
    /// assert_eq!(port.as_str(), "80");
    /// ```
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns a u16 value of the `Port`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::{Authority, Port};
    ///
    /// let authority = Authority::from_bytes(b"example.com:80").unwrap();
    /// let port = authority.port().unwrap();
    /// assert_eq!(port.as_u16().unwrap(), 80);
    /// ```
    pub fn as_u16(&self) -> Result<u16, HttpError> {
        self.0
            .parse::<u16>()
            .map_err(|_| ErrorKind::Uri(InvalidUri::InvalidPort).into())
    }
}

/// Path component of [`Uri`].
///
/// [`Uri`]: Uri
#[derive(Clone, Debug, Default)]
pub struct Path(String);

impl Path {
    /// Converts a byte slice into a `Path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Path;
    ///
    /// let path = Path::from_bytes(b"/foo").unwrap();
    /// assert_eq!(path.as_str(), "/foo");
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Path, InvalidUri> {
        let (path, rest) = path_token(bytes)?;
        if rest.is_empty() {
            path.ok_or(InvalidUri::UriMissPath)
        } else {
            Err(InvalidUri::InvalidPath)
        }
    }

    /// Returns a string slice containing the entire `Path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Path;
    ///
    /// let path = Path::from_bytes(b"/foo").unwrap();
    /// assert_eq!(path.as_str(), "/foo");
    /// ```
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl<'a> TryFrom<&'a [u8]> for Path {
    type Error = InvalidUri;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        Path::from_bytes(bytes)
    }
}

impl<'a> TryFrom<&'a str> for Path {
    type Error = InvalidUri;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        TryFrom::try_from(s.as_bytes())
    }
}

/// Query component of [`Uri`].
///
/// [`Uri`]: Uri
#[derive(Clone, Debug, Default)]
pub struct Query(String);

impl Query {
    /// Converts a byte slice into a `Query`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Query;
    ///
    /// let query = Query::from_bytes(b"a=1").unwrap();
    /// assert_eq!(query.as_str(), "a=1");
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Query, InvalidUri> {
        let query = query_token(bytes)?;
        query.ok_or(InvalidUri::UriMissQuery)
    }

    /// Returns a string slice containing the entire `Query`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::uri::Query;
    ///
    /// let query = Query::from_bytes(b"a=1").unwrap();
    /// assert_eq!(query.as_str(), "a=1");
    /// ```
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl<'a> TryFrom<&'a [u8]> for Query {
    type Error = InvalidUri;

    fn try_from(s: &'a [u8]) -> Result<Self, Self::Error> {
        Query::from_bytes(s)
    }
}

impl<'a> TryFrom<&'a str> for Query {
    type Error = InvalidUri;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        TryFrom::try_from(s.as_bytes())
    }
}

/// `Protocol` indicates the scheme type supported by [`Uri`].
///
/// [`Uri`]: Uri
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum Protocol {
    Http,
    Https,
}

/// Error types generated during [`Uri`] construction due to different causes.
///
/// [`Uri`]: Uri
#[derive(Debug, Eq, PartialEq)]
pub enum InvalidUri {
    /// Invalid scheme
    InvalidScheme,
    /// Invalid authority
    InvalidAuthority,
    /// Invalid path
    InvalidPath,
    /// Invalid byte
    InvalidByte,
    /// Invalid format
    InvalidFormat,
    /// Invalid port
    InvalidPort,
    /// Missing scheme
    UriMissScheme,
    /// Missing path
    UriMissPath,
    /// Missing query
    UriMissQuery,
    /// Missing authority
    UriMissAuthority,
    /// Missing host
    UriMissHost,
    /// Contains Userinfo
    UriContainUserinfo,
    /// Too long
    UriTooLong,
}

impl From<Infallible> for InvalidUri {
    fn from(_: Infallible) -> Self {
        unimplemented!()
    }
}

fn bytes_to_str(bytes: &[u8]) -> &str {
    unsafe { std::str::from_utf8_unchecked(bytes) }
}

fn scheme_token(bytes: &[u8]) -> Result<(Option<Scheme>, &[u8]), InvalidUri> {
    const HTTP_SCHEME_LENGTH: usize = "http://".len();
    const HTTPS_SCHEME_LENGTH: usize = "https://".len();
    // Obtains the position of colons that separate schemes.
    let pos = match bytes.iter().enumerate().find(|(_, &b)| b == b':') {
        Some((index, _))
            if index != 0
                && bytes[index..].len() > 2
                && bytes[index + 1..index + 3].eq_ignore_ascii_case(b"//") =>
        {
            index
        }
        Some((0, _)) => return Err(InvalidUri::InvalidScheme),
        _ => return Ok((None, bytes)),
    };
    // Currently, only HTTP and HTTPS are supported. Therefore, you need to verify
    // the scheme content.
    if bytes[..pos].eq_ignore_ascii_case(b"http") {
        Ok((Some(Protocol::Http.into()), &bytes[HTTP_SCHEME_LENGTH..]))
    } else if bytes[..pos].eq_ignore_ascii_case(b"https") {
        Ok((Some(Protocol::Https.into()), &bytes[HTTPS_SCHEME_LENGTH..]))
    } else {
        Err(InvalidUri::InvalidScheme)
    }
}

fn authority_token(bytes: &[u8]) -> Result<(Option<Authority>, &[u8]), InvalidUri> {
    let mut end = bytes.len();
    let mut colon_num = 0;
    let mut left_bracket = false;
    let mut right_bracket = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'/' | b'?' | b'#' => {
                end = i;
                break;
            }
            b'[' => {
                if i == 0 {
                    left_bracket = true;
                } else if left_bracket {
                    return Err(InvalidUri::InvalidAuthority);
                }
            }
            b']' => {
                if left_bracket {
                    if right_bracket {
                        return Err(InvalidUri::InvalidAuthority);
                    } else {
                        right_bracket = true;
                        // The ':' between '[' and ']' is in ipv6 and should be ignored.
                        colon_num = 0;
                    }
                }
            }
            // TODO According to RFC3986, the character @ can be one of the reserved characters,
            // which needs to be improved after being familiar with the rules.
            b'@' => {
                return Err(InvalidUri::UriContainUserinfo);
            }
            b':' => {
                colon_num += 1;
            }
            other => {
                if !URI_VALUE_BYTES[other as usize] {
                    return Err(InvalidUri::InvalidByte);
                }
            }
        }
    }
    // The authority does not exist.
    if end == 0 {
        return Ok((None, &bytes[end..]));
    }

    // Incomplete square brackets
    if left_bracket ^ right_bracket {
        return Err(InvalidUri::InvalidAuthority);
    }
    // There are multiple colons in addition to IPv6.
    if colon_num > 1 {
        return Err(InvalidUri::InvalidAuthority);
    }
    let authority = host_port(&bytes[..end], colon_num)?;
    Ok((Some(authority), &bytes[end..]))
}

fn path_token(bytes: &[u8]) -> Result<(Option<Path>, &[u8]), InvalidUri> {
    let mut end = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'?' | b'#' => {
                end = i;
                break;
            }
            _ => {
                if !URI_VALUE_BYTES[b as usize] {
                    return Err(InvalidUri::InvalidByte);
                }
            }
        }
    }
    if end != 0 {
        let path = bytes_to_str(&bytes[..end]).to_string();
        Ok((Some(Path(path)), &bytes[end..]))
    } else {
        Ok((None, &bytes[end..]))
    }
}

fn query_token(s: &[u8]) -> Result<Option<Query>, InvalidUri> {
    if s.is_empty() {
        return Ok(None);
    }
    let bytes = if s[0].eq_ignore_ascii_case(&b'?') {
        &s[1..]
    } else {
        s
    };
    let mut end = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'#' => {
                end = i;
                break;
            }
            _ => {
                if !URI_VALUE_BYTES[b as usize] {
                    return Err(InvalidUri::InvalidByte);
                }
            }
        }
    }
    if end == 0 {
        return Ok(None);
    }
    let query = bytes_to_str(&bytes[..end]);
    Ok(Some(Query(query.to_string())))
}

fn host_port(auth: &[u8], colon_num: i32) -> Result<Authority, InvalidUri> {
    let authority = bytes_to_str(auth);
    if colon_num != 0 {
        match authority.rsplit_once(':') {
            Some((host, port)) => {
                if host.is_empty() {
                    Err(InvalidUri::UriMissHost)
                } else if port.is_empty() {
                    Ok(Authority {
                        host: Host(host.to_string()),
                        port: None,
                    })
                } else {
                    port.parse::<u16>().map_err(|_| InvalidUri::InvalidPort)?;
                    Ok(Authority {
                        host: Host(host.to_string()),
                        port: Some(Port(port.to_string())),
                    })
                }
            }
            None => Err(InvalidUri::UriMissAuthority),
        }
    } else {
        Ok(Authority {
            host: Host(authority.to_string()),
            port: None,
        })
    }
}

fn validity_check(unchecked_uri: Uri) -> Result<Uri, InvalidUri> {
    match (
        &unchecked_uri.scheme,
        &unchecked_uri.authority,
        &unchecked_uri.path,
        &unchecked_uri.query,
    ) {
        (Some(_), None, _, _) => Err(InvalidUri::UriMissAuthority),
        (None, Some(_), Some(_), _) => Err(InvalidUri::UriMissScheme),
        (None, Some(_), _, Some(_)) => Err(InvalidUri::UriMissScheme),
        (None, None, None, None) => Err(InvalidUri::InvalidFormat),
        _ => Ok(unchecked_uri),
    }
}

#[rustfmt::skip]
const URI_VALUE_BYTES: [bool; 256] = {
    const __: bool = false;
    const TT: bool = true;
    [
//      \0                                  HT  LF          CR
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 1F
//      \w  !   "   #   $   %   &   '   (   )   *   +   ,   -   .   /
        __, TT, __, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, // 2F
//      0   1   2   3   4   5   6   7   8   9   :   ;   <   =   >   ?
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, TT, __, TT, // 3F
//      @   A   B   C   D   E   F   G   H   I   J   K   L   M   N   O
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, // 4F
//      P   Q   R   S   T   U   V   W   X   Y   Z   [   \   ]   ^   _
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, TT, __, TT, // 5F
//      `   a   b   c   d   e   f   g   h   i   j   k   l   m   n   o
        __, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, // 6F
//      p   q   r   s   t   u   v   w   x   y   z   {   |   }   ~   del
        TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, TT, __, __, __, TT, __, // 7F
//      Expand ascii
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8F
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9F
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // AF
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // BF
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // CF
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // DF
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // EF
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // FF
    ]
};

#[cfg(test)]
mod ut_uri {
    use super::{InvalidUri, Scheme, Uri, UriBuilder};
    use crate::error::{ErrorKind, HttpError};

    macro_rules! test_builder_valid {
        ($res1:expr, $res2:expr) => {{
            let uri = UriBuilder::new()
                .scheme($res1.0)
                .authority($res1.1)
                .path($res1.2)
                .query($res1.3)
                .build()
                .unwrap();
            assert_eq!(uri.scheme().unwrap().as_str(), $res2.0);
            assert_eq!(uri.host().unwrap().as_str(), $res2.1);
            assert_eq!(uri.port().unwrap().as_str(), $res2.2);
            assert_eq!(uri.path().unwrap().as_str(), $res2.3);
            assert_eq!(uri.query().unwrap().as_str(), $res2.4);
            assert_eq!(uri.to_string(), $res2.5)
        }};
    }

    /// UT test cases for `build_from_builder`.
    ///
    /// # Brief
    /// 1. Creates UriBuilder by calling UriBuilder::new().
    /// 2. Sets Scheme by calling scheme().
    /// 3. Sets authority by calling authority().
    /// 4. Sets path by calling path().
    /// 5. Sets query by calling query().
    /// 6. Creates Uri by calling build().
    /// 7. Gets string slice value of uri components by calling as_str().
    /// 8. Gets string value of uri by calling to_string().
    /// 9. Checks if the test result is correct by assert_eq!().
    #[test]
    fn build_from_builder() {
        test_builder_valid!(
            ("http", "hyper.rs:80", "/foo", "a=1"),
            (
                "http",
                "hyper.rs",
                "80",
                "/foo",
                "a=1",
                "http://hyper.rs:80/foo?a=1"
            )
        );
        test_builder_valid!(
            (Scheme::HTTP, "hyper.rs:80", "/foo", "a=1"),
            (
                "http",
                "hyper.rs",
                "80",
                "/foo",
                "a=1",
                "http://hyper.rs:80/foo?a=1"
            )
        );
        test_builder_valid!(
            ("https", "hyper.rs:80", "/foo", "a=1"),
            (
                "https",
                "hyper.rs",
                "80",
                "/foo",
                "a=1",
                "https://hyper.rs:80/foo?a=1"
            )
        );
        test_builder_valid!(
            (Scheme::HTTPS, "hyper.rs:80", "/foo", "a=1"),
            (
                "https",
                "hyper.rs",
                "80",
                "/foo",
                "a=1",
                "https://hyper.rs:80/foo?a=1"
            )
        );
    }

    /// UT test cases for `build_from_instance`.
    ///
    /// # Brief
    /// 1. Creates UriBuilder by calling Uri::builder().
    /// 2. Sets Scheme by calling scheme().
    /// 3. Sets authority by calling authority().
    /// 4. Sets path by calling path().
    /// 5. Sets query by calling query().
    /// 6. Creates Uri by calling build().
    /// 7. Gets string slice value of uri components by calling as_str().
    /// 8. Gets string value of uri by calling to_string().
    /// 9. Checks if the test result is correct by assert_eq!().
    #[test]
    fn build_from_instance() {
        let uri = Uri::builder()
            .scheme(Scheme::HTTP)
            .authority("hyper.rs:80")
            .path("/foo")
            .query("a=1")
            .build()
            .unwrap();
        assert_eq!(uri.scheme().unwrap().as_str(), "http");
        assert_eq!(uri.host().unwrap().as_str(), "hyper.rs");
        assert_eq!(uri.port().unwrap().as_str(), "80");
        assert_eq!(uri.path().unwrap().as_str(), "/foo");
        assert_eq!(uri.query().unwrap().as_str(), "a=1");
        assert_eq!(uri.to_string(), "http://hyper.rs:80/foo?a=1")
    }

    /// UT test cases for `build_from_str`.
    ///
    /// # Brief
    /// 1. Creates Uri by calling from_bytes().
    /// 2. Gets string slice value of uri components by calling as_str().
    /// 3. Gets u16 value of port by call as_u16().
    /// 4. Gets string value of uri by calling to_string().
    /// 5. Checks if the test result is correct by assert_eq!().
    #[test]
    fn build_from_str() {
        let uri = Uri::from_bytes("http://hyper.rs:80/foo?a=1".as_bytes()).unwrap();
        assert_eq!(uri.scheme().unwrap().as_str(), "http");
        assert_eq!(uri.host().unwrap().as_str(), "hyper.rs");
        assert_eq!(uri.port().unwrap().as_str(), "80");
        assert_eq!(uri.port().unwrap().as_u16().unwrap(), 80);
        assert_eq!(uri.path().unwrap().as_str(), "/foo");
        assert_eq!(uri.query().unwrap().as_str(), "a=1");
        assert_eq!(uri.to_string(), "http://hyper.rs:80/foo?a=1")
    }

    /// UT test cases for `Scheme::default_port`.
    ///
    /// # Brief
    /// 1. Creates Scheme by calling `Scheme::from_bytes`.
    /// 3. Gets u16 value of port by calling `Scheme::default_port`.
    /// 5. Checks whether the default port is correct.
    #[test]
    fn ut_uri_scheme_default_port() {
        let scheme = Scheme::from_bytes(b"http").unwrap();
        assert_eq!(scheme.default_port(), 80);
        let scheme = Scheme::from_bytes(b"https").unwrap();
        assert_eq!(scheme.default_port(), 443);
    }

    /// UT test cases for `Uri::from_bytes`.
    ///
    /// # Brief
    /// 1. Creates Uri by calling `Uri::from_bytes()`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_uri_from_bytes() {
        macro_rules! uri_test_case {
            ($raw: expr, $tar: expr, $(,)?) => {
                match (Uri::from_bytes($raw), $tar) {
                    (Ok(res), Ok(tar)) => assert_eq!(
                        (
                            res.scheme().map(|scheme| scheme.as_str()),
                            res.host().map(|host| host.as_str()),
                            res.port().map(|port| port.as_str()),
                            res.path().map(|path| path.as_str()),
                            res.query().map(|query| query.as_str()),
                        ),
                        tar,
                    ),
                    (Err(res), Err(tar)) => assert_eq!(res, tar),
                    _ => panic!("uri test case failed!"),
                }
            };
        }

        uri_test_case!(
            b"httpss://www.example.com/",
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidScheme))),
        );

        uri_test_case!(
            b"://www.example.com/",
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidScheme))),
        );

        uri_test_case!(
            b"https://www.hu awei.com/",
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br#"https://www.hu"awei.com/"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br#"https://www.hu"awei.com/"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br#"https://www.hu"<>\^`awei.com/"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br#"https://www.example.com:a0/"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidPort))),
        );

        uri_test_case!(
            br#"https://www.example.com:80/message/e<>mail"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br"https://www.example.com:80/message/email?name='\^'",
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::InvalidByte))),
        );

        uri_test_case!(
            br#"https:/www.example.com:80/message/email?name=arya"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::UriMissScheme))),
        );

        uri_test_case!(
            br#"https:/www.example.com:80"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::UriMissScheme))),
        );

        uri_test_case!(
            br#"https:/www.example.com"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::UriMissScheme))),
        );

        uri_test_case!(
            br#"https://www.huaw:ei.com:80"#,
            Err(HttpError::from(ErrorKind::Uri(
                InvalidUri::InvalidAuthority
            ))),
        );

        uri_test_case!(
            br#"https://www.huaw:ei.com:80"#,
            Err(HttpError::from(ErrorKind::Uri(
                InvalidUri::InvalidAuthority
            ))),
        );

        uri_test_case!(
            br#"https://name=1234@www.example.com:80/message/email?name=arya"#,
            Err(HttpError::from(ErrorKind::Uri(
                InvalidUri::UriContainUserinfo
            ))),
        );

        uri_test_case!(
            br#"www.example.com:80/message/email?name=arya"#,
            Err(HttpError::from(ErrorKind::Uri(InvalidUri::UriMissScheme))),
        );

        uri_test_case!(
            br#"https://[0:0:0:0:0:0:0:0:80/message/email?name=arya"#,
            Err(HttpError::from(ErrorKind::Uri(
                InvalidUri::InvalidAuthority
            ))),
        );

        uri_test_case!(
            br#"https:///foo?a=1"#,
            Err(HttpError::from(ErrorKind::Uri(
                InvalidUri::UriMissAuthority
            ))),
        );

        uri_test_case!(
            b"https://www.example.com/",
            Ok((
                Some("https"),
                Some("www.example.com"),
                None,
                Some("/"),
                None
            )),
        );

        uri_test_case!(
            b"https://www.example.com:80/foo?a=1",
            Ok((
                Some("https"),
                Some("www.example.com"),
                Some("80"),
                Some("/foo"),
                Some("a=1"),
            )),
        );

        uri_test_case!(
            b"https://www.example.com:80/foo?a=1#fragment",
            Ok((
                Some("https"),
                Some("www.example.com"),
                Some("80"),
                Some("/foo"),
                Some("a=1"),
            )),
        );

        uri_test_case!(
            b"https://www.example.com:80?a=1",
            Ok((
                Some("https"),
                Some("www.example.com"),
                Some("80"),
                None,
                Some("a=1"),
            )),
        );

        uri_test_case!(
            b"https://www.example.com?a=1",
            Ok((
                Some("https"),
                Some("www.example.com"),
                None,
                None,
                Some("a=1"),
            )),
        );

        uri_test_case!(
            b"https://www.example.com?",
            Ok((Some("https"), Some("www.example.com"), None, None, None)),
        );

        uri_test_case!(
            b"https://www.example.com:80",
            Ok((
                Some("https"),
                Some("www.example.com"),
                Some("80"),
                None,
                None,
            )),
        );

        uri_test_case!(
            b"https://www.example.com",
            Ok((Some("https"), Some("www.example.com"), None, None, None)),
        );

        uri_test_case!(
            b"https://www.example.com#fragment",
            Ok((Some("https"), Some("www.example.com"), None, None, None)),
        );

        uri_test_case!(
            b"www.example.com",
            Ok((None, Some("www.example.com"), None, None, None)),
        );

        uri_test_case!(
            b"/foo?a=1",
            Ok((None, None, None, Some("/foo"), Some("a=1"))),
        );

        uri_test_case!(
            b"https://[0:0:0:0:0:0:0:0]",
            Ok((Some("https"), Some("[0:0:0:0:0:0:0:0]"), None, None, None)),
        );

        uri_test_case!(
            b"https://[0:0:0:0:0:0:0:0]:80",
            Ok((
                Some("https"),
                Some("[0:0:0:0:0:0:0:0]"),
                Some("80"),
                None,
                None,
            )),
        );
    }

    /// UT test cases for `Uri::authority`.
    ///
    /// # Brief
    /// 1. Creates Uri by calling `Uri::authority()`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_uri_authority() {
        let uri = Uri::from_bytes(b"http://example.com:8080/foo?a=1").unwrap();
        let authority = uri.authority().unwrap();
        assert_eq!(authority.host.as_str(), "example.com");
        assert_eq!(authority.port().unwrap().as_str(), "8080");
    }

    /// UT test cases for `Uri::path_and_query`.
    ///
    /// # Brief
    /// 1. Creates Uri by calling `Uri::path_and_query()`.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_uri_path_and_query() {
        let uri = Uri::from_bytes(b"http://example.com:8080/foo?a=1").unwrap();
        assert_eq!(uri.path_and_query().unwrap(), "/foo?a=1");

        let uri = Uri::from_bytes(b"http://example.com:8080").unwrap();
        assert_eq!(uri.path_and_query(), None);
    }
}
