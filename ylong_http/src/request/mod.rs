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

//! HTTP [`Request`][http_request].
//!
//! This module provides [`Request`][my_request], [`RequestBuilder`] and
//! [`RequestPart`].
//!
//! [http_request]: https://www.rfc-editor.org/rfc/rfc9112.html#request.line
//! [my_request]: Request
//! [`RequestBuilder`]: RequestBuilder
//! [`RequestPart`]: RequestPart
//!
//! # Examples
//!
//! ```
//! use ylong_http::request::method::Method;
//! use ylong_http::request::{Request, RequestBuilder};
//! use ylong_http::version::Version;
//!
//! // Uses `RequestBuilder` to construct a `Request`.
//! let request = Request::builder()
//!     .method("GET")
//!     .url("www.example.com")
//!     .version("HTTP/1.1")
//!     .header("ACCEPT", "text/html")
//!     .append_header("ACCEPT", "application/xml")
//!     .body(())
//!     .unwrap();
//!
//! assert_eq!(request.method(), &Method::GET);
//! assert_eq!(request.uri().to_string(), "www.example.com");
//! assert_eq!(request.version(), &Version::HTTP1_1);
//! assert_eq!(
//!     request
//!         .headers()
//!         .get("accept")
//!         .unwrap()
//!         .to_string()
//!         .unwrap(),
//!     "text/html, application/xml"
//! );
//! ```

pub mod method;
pub mod uri;

use core::convert::TryFrom;

use method::Method;
use uri::Uri;

#[cfg(any(feature = "ylong_base", feature = "tokio_base"))]
use crate::body::{MultiPart, MultiPartBase};
use crate::error::{ErrorKind, HttpError};
use crate::headers::{Header, HeaderName, HeaderValue, Headers};
use crate::version::Version;

/// HTTP `Request`. A `Request` consists of a request line and a body.
///
/// # Examples
///
/// ```
/// use ylong_http::request::Request;
///
/// let request = Request::new("this is a body");
/// assert_eq!(request.body(), &"this is a body");
/// ```
pub struct Request<T> {
    part: RequestPart,
    body: T,
}

impl Request<()> {
    /// Creates a new, default `RequestBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let builder = Request::builder();
    /// ```
    pub fn builder() -> RequestBuilder {
        RequestBuilder::new()
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to `GET`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::get("www.example.com").body(()).unwrap();
    /// ```
    pub fn get<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::GET).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `HEAD`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::head("www.example.com").body(()).unwrap();
    /// ```
    pub fn head<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::HEAD).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `POST`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::post("www.example.com").body(()).unwrap();
    /// ```
    pub fn post<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::POST).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to `PUT`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::put("www.example.com").body(()).unwrap();
    /// ```
    pub fn put<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::PUT).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `DELETE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::delete("www.example.com").body(()).unwrap();
    /// ```
    pub fn delete<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::DELETE).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `CONNECT`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::connect("www.example.com").body(()).unwrap();
    /// ```
    pub fn connect<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::CONNECT).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `OPTIONS`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::options("www.example.com").body(()).unwrap();
    /// ```
    pub fn options<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        RequestBuilder::new().method(Method::OPTIONS).url(uri)
    }

    /// Creates a `RequestBuilder` for the given `Uri` with method set to
    /// `TRACE`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::trace("www.example.com").body(()).unwrap();
    /// ```
    pub fn trace<T>(uri: T) -> RequestBuilder
    where
        Uri: TryFrom<T>,
        HttpError: From<<Uri as TryFrom<T>>::Error>,
    {
        RequestBuilder::new().method(Method::TRACE).url(uri)
    }
}

impl<T> Request<T> {
    /// Creates a new, default `Request` with options set default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new();
    /// ```
    pub fn new(body: T) -> Self {
        Request {
            part: Default::default(),
            body,
        }
    }

    /// Gets an immutable reference to the `Method`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let method = request.method();
    /// ```
    pub fn method(&self) -> &Method {
        &self.part.method
    }

    /// Gets a mutable reference to the `Method`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let method = request.method_mut();
    /// ```
    pub fn method_mut(&mut self) -> &mut Method {
        &mut self.part.method
    }

    /// Gets an immutable reference to the `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let uri = request.uri();
    /// ```
    pub fn uri(&self) -> &Uri {
        &self.part.uri
    }

    /// Gets a mutable reference to the `Uri`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let uri = request.uri_mut();
    /// ```
    pub fn uri_mut(&mut self) -> &mut Uri {
        &mut self.part.uri
    }

    /// Gets an immutable reference to the `Version`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let version = request.version();
    /// ```
    pub fn version(&self) -> &Version {
        &self.part.version
    }

    /// Gets a mutable reference to the `Version`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let version = request.version_mut();
    /// ```
    pub fn version_mut(&mut self) -> &mut Version {
        &mut self.part.version
    }

    /// Gets an immutable reference to the `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let headers = request.headers();
    /// ```
    pub fn headers(&self) -> &Headers {
        &self.part.headers
    }

    /// Gets a mutable reference to the `Headers`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let headers = request.headers_mut();
    /// ```
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.part.headers
    }

    /// Gets an immutable reference to the `RequestPart`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let part = request.part();
    /// ```
    pub fn part(&self) -> &RequestPart {
        &self.part
    }

    /// Gets a mutable reference to the `RequestPart`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let part = request.part_mut();
    /// ```
    pub fn part_mut(&mut self) -> &RequestPart {
        &mut self.part
    }

    /// Gets an immutable reference to the `Body`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let request = Request::new(());
    /// let body = request.body();
    /// ```
    pub fn body(&self) -> &T {
        &self.body
    }

    /// Gets a mutable reference to the `Body`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::Request;
    ///
    /// let mut request = Request::new(());
    /// let body = request.body_mut();
    /// ```
    pub fn body_mut(&mut self) -> &mut T {
        &mut self.body
    }

    /// Splits `Request` into `RequestPart` and `Body`.
    ///
    /// # Examples
    /// ```
    /// use ylong_http::request::{Request, RequestPart};
    ///
    /// let request = Request::new(());
    /// let (part, body) = request.into_parts();
    /// ```
    pub fn into_parts(self) -> (RequestPart, T) {
        (self.part, self.body)
    }

    /// Combines `RequestPart` and `Body` into a `Request`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::{Request, RequestPart};
    ///
    /// let part = RequestPart::default();
    /// let body = ();
    /// let request = Request::from_raw_parts(part, body);
    /// ```
    pub fn from_raw_parts(part: RequestPart, body: T) -> Request<T> {
        Request { part, body }
    }
}

impl<T: Clone> Clone for Request<T> {
    fn clone(&self) -> Self {
        Request::from_raw_parts(self.part.clone(), self.body.clone())
    }
}

/// A builder which is used to construct `Request`.
///
/// # Examples
///
/// ```
/// use ylong_http::headers::Headers;
/// use ylong_http::request::method::Method;
/// use ylong_http::request::RequestBuilder;
/// use ylong_http::version::Version;
///
/// let request = RequestBuilder::new()
///     .method("GET")
///     .url("www.example.com")
///     .version("HTTP/1.1")
///     .header("ACCEPT", "text/html")
///     .append_header("ACCEPT", "application/xml")
///     .body(())
///     .unwrap();
///
/// assert_eq!(request.method(), &Method::GET);
/// assert_eq!(request.uri().to_string(), "www.example.com");
/// assert_eq!(request.version(), &Version::HTTP1_1);
/// assert_eq!(
///     request
///         .headers()
///         .get("accept")
///         .unwrap()
///         .to_string()
///         .unwrap(),
///     "text/html, application/xml"
/// );
/// ```
pub struct RequestBuilder {
    part: Result<RequestPart, HttpError>,
}

impl RequestBuilder {
    /// Creates a new, default `RequestBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new();
    /// ```
    pub fn new() -> Self {
        RequestBuilder {
            part: Ok(RequestPart::default()),
        }
    }

    /// Sets the `Method` of the `Request`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().method("GET");
    /// ```
    pub fn method<T>(mut self, method: T) -> Self
    where
        Method: TryFrom<T>,
        <Method as TryFrom<T>>::Error: Into<HttpError>,
    {
        self.part = self.part.and_then(move |mut part| {
            part.method = Method::try_from(method).map_err(Into::into)?;
            Ok(part)
        });
        self
    }

    /// Sets the `Uri` of the `Request`. `Uri` does not provide a default value,
    /// so it must be set.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().url("www.example.com");
    /// ```
    pub fn url<T>(mut self, uri: T) -> Self
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        self.part = self.part.and_then(move |mut part| {
            part.uri = Uri::try_from(uri).map_err(Into::into)?;
            Ok(part)
        });
        self
    }

    /// Sets the `Version` of the `Request`. Uses `Version::HTTP1_1` by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let request = RequestBuilder::new().version("HTTP/1.1");
    /// ```
    pub fn version<T>(mut self, version: T) -> Self
    where
        Version: TryFrom<T>,
        <Version as TryFrom<T>>::Error: Into<HttpError>,
    {
        self.part = self.part.and_then(move |mut part| {
            part.version = Version::try_from(version).map_err(Into::into)?;
            Ok(part)
        });
        self
    }

    /// Adds a `Header` to `Request`. Overwrites `HeaderValue` if the
    /// `HeaderName` already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let request = RequestBuilder::new().header("ACCEPT", "text/html");
    /// ```
    pub fn header<N, V>(mut self, name: N, value: V) -> Self
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.part = self.part.and_then(move |mut part| {
            part.headers.insert(name, value)?;
            Ok(part)
        });
        self
    }

    /// Adds a `Header` to `Request`. Appends `HeaderValue` to the end of
    /// previous `HeaderValue` if the `HeaderName` already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::headers::Headers;
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let request = RequestBuilder::new().append_header("ACCEPT", "text/html");
    /// ```
    pub fn append_header<N, V>(mut self, name: N, value: V) -> Self
    where
        HeaderName: TryFrom<N>,
        <HeaderName as TryFrom<N>>::Error: Into<HttpError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpError>,
    {
        self.part = self.part.and_then(move |mut part| {
            part.headers.append(name, value)?;
            Ok(part)
        });
        self
    }

    /// Try to create a `Request` based on the incoming `body`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::request::RequestBuilder;
    ///
    /// let request = RequestBuilder::new().body(()).unwrap();
    /// ```
    pub fn body<T>(self, body: T) -> Result<Request<T>, HttpError> {
        Ok(Request {
            part: self.part?,
            body,
        })
    }
}

impl Default for RequestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// `RequestPart`, which is called [`Request Line`] in [`RFC9112`].
///
/// A request-line begins with a method token, followed by a single space (SP),
/// the request-target, and another single space (SP), and ends with the
/// protocol version.
///
/// [`RFC9112`]: https://httpwg.org/specs/rfc9112.html
/// [`Request Line`]: https://httpwg.org/specs/rfc9112.html#request.line
///
/// # Examples
///
/// ```
/// use ylong_http::request::Request;
///
/// let request = Request::new(());
///
/// // Uses `Request::into_parts` to get a `RequestPart`.
/// let (part, _) = request.into_parts();
/// ```
#[derive(Clone, Debug)]
pub struct RequestPart {
    /// HTTP URI implementation
    pub uri: Uri,
    /// HTTP Method implementation
    pub method: Method,
    /// HTTP Version implementation
    pub version: Version,
    /// HTTP Headers, which is called Fields in RFC9110.
    pub headers: Headers,
}

impl Default for RequestPart {
    fn default() -> Self {
        Self {
            uri: Uri::http(),
            method: Method::GET,
            version: Version::HTTP1_1,
            headers: Headers::new(),
        }
    }
}

#[cfg(test)]
mod ut_request {
    use core::convert::TryFrom;

    use super::{Method, Request, RequestBuilder, RequestPart, Uri};
    use crate::headers::Headers;
    use crate::version::Version;

    /// UT test cases for `RequestBuilder::build`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `RequestBuilder::build`.
    /// 2. Sets method by calling `RequestBuilder::method`.
    /// 3. Sets uri by calling `RequestBuilder::uri`.
    /// 4. Sets version by calling `RequestBuilder::version`.
    /// 5. Sets header by calling `RequestBuilder::insert_header`.
    /// 6. Sets header by calling `RequestBuilder::append_header`.
    /// 7. Gets method by calling `Request::method`.
    /// 8. Gets uri by calling `Request::uri`.
    /// 9. Gets version by calling `Request::version`.
    /// 10. Gets headers by calling `Request::headers`.
    /// 11. Checks if the test result is correct.
    #[test]
    fn ut_request_builder_build() {
        let request = RequestBuilder::new()
            .method("GET")
            .url("www.baidu.com")
            .version("HTTP/1.1")
            .header("ACCEPT", "text/html")
            .append_header("ACCEPT", "application/xml")
            .body(())
            .unwrap();

        let mut new_headers = Headers::new();
        let _ = new_headers.insert("accept", "text/html");
        let _ = new_headers.append("accept", "application/xml");

        assert_eq!(request.method().as_str(), "GET");
        assert_eq!(request.uri().to_string().as_str(), "www.baidu.com");
        assert_eq!(request.version().as_str(), "HTTP/1.1");
        assert_eq!(request.headers(), &new_headers);
    }

    /// UT test cases for `RequestBuilder::build`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `RequestBuilder::build`.
    /// 2. Sets method by calling `RequestBuilder.method`.
    /// 3. Sets uri by calling `RequestBuilder.uri`.
    /// 4. Sets version by calling `RequestBuilder.version`.
    /// 5. Sets header by calling `RequestBuilder.insert_header`.
    /// 6. Sets header by calling `RequestBuilder.append_header`.
    /// 7. Changes method by calling `Request.method_mut`.
    /// 8. Changes uri by calling `Request.uri_mut`.
    /// 9. Changes version by calling `Request.version_mut`.
    /// 10. Changes headers by calling `Request.headers_mut`.
    /// 11. Gets method by calling `Request.method`.
    /// 12. Gets uri by calling `Request.uri`.
    /// 13. Gets version by calling `Request.version`.
    /// 14. Gets headers by calling `Request.headers`.
    /// 15. Checks if the test result is correct.
    #[test]
    fn ut_request_builder_build_2() {
        let mut request = RequestBuilder::new()
            .method("GET")
            .url("www.example.com")
            .version("HTTP/1.1")
            .header("ACCEPT", "text/html")
            .body(())
            .unwrap();

        *request.method_mut() = Method::POST;
        *request.uri_mut() = Uri::try_from("www.test.com").unwrap();
        *request.version_mut() = Version::HTTP2;
        let _ = request.headers_mut().insert("accept", "application/xml");

        let mut new_headers = Headers::new();
        let _ = new_headers.insert("accept", "application/xml");

        assert_eq!(request.method().as_str(), "POST");
        assert_eq!(request.uri().to_string().as_str(), "www.test.com");
        assert_eq!(request.version().as_str(), "HTTP/2.0");
        assert_eq!(request.headers(), &new_headers);
    }

    /// UT test cases for `Request::new`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `Request::new`.
    /// 2. Gets body by calling `Request.body`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_new() {
        let request = Request::new(String::from("<body><div></div></body>"));
        assert_eq!(
            request.body().to_owned().as_str(),
            "<body><div></div></body>"
        );
    }

    /// UT test cases for `Request::into_parts`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `Request::new`.
    /// 2. Gets request part and body by calling `Request.into_parts`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_into_parts() {
        let request = Request::new(String::from("<body><div></div></body>"));
        let (part, body) = request.into_parts();
        assert_eq!(part.method.as_str(), "GET");
        assert_eq!(body.as_str(), "<body><div></div></body>");
    }

    /// UT test cases for `Request::part`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `Request::new`.
    /// 2. Gets request part and body by calling `Request.part`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_part() {
        let request = Request::new(());
        let part = request.part();
        assert_eq!(part.method.as_str(), "GET");
        assert_eq!(part.version.as_str(), "HTTP/1.1");
    }

    /// UT test cases for `Request::from_raw_parts`.
    ///
    /// # Brief
    /// 1. Creates a `RequestPart` and a body.
    /// 2. Gets the request by calling `Request::from_raw_parts`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_from_raw_parts() {
        let part = RequestPart::default();
        let body = String::from("<body><div></div></body>");
        let request = Request::from_raw_parts(part, body);
        assert_eq!(request.part.method.as_str(), "GET");
        assert_eq!(request.body, "<body><div></div></body>");
    }

    /// UT test cases for `Request::get`.
    ///
    /// # Brief
    /// 1. Creates a `Request` by calling `Request::get`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_request_get() {
        let request = Request::get("www.example.com").body("".as_bytes()).unwrap();
        assert_eq!(request.part.uri.to_string(), "www.example.com");
        assert_eq!(request.part.method.as_str(), "GET");
        assert_eq!(request.part.version.as_str(), "HTTP/1.1");
    }
}
