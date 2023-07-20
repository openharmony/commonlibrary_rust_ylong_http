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

use std::convert::TryFrom;
use std::ops::{Deref, DerefMut};

use ylong_http::body::async_impl::Body;
use ylong_http::body::MultiPart;
use ylong_http::error::HttpError;
use ylong_http::headers::{HeaderName, HeaderValue};
use ylong_http::request::method::Method;
use ylong_http::request::uri::Uri;
use ylong_http::request::{Request, RequestBuilder as ReqBuilder};
use ylong_http::response::Response as Resp;
use ylong_http::version::Version;

use crate::async_impl::HttpBody;
use crate::{ErrorKind, HttpClientError};

/// Response Adapter.
pub struct Response {
    response: Resp<HttpBody>,
}

impl Response {
    pub(crate) fn new(response: Resp<HttpBody>) -> Self {
        Self { response }
    }

    /// `text()` adapter.
    pub async fn text(self) -> Result<String, HttpClientError> {
        let mut buf = [0u8; 1024];
        let mut vec = Vec::new();
        let mut response = self.response;
        loop {
            let size = response.body_mut().data(&mut buf).await?;
            if size == 0 {
                break;
            }
            vec.extend_from_slice(&buf[..size]);
        }
        String::from_utf8(vec).map_err(|_| {
            HttpClientError::new_with_message(
                ErrorKind::BodyDecode,
                "The body content is not valid utf8.",
            )
        })
    }
}

impl Deref for Response {
    type Target = Resp<HttpBody>;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl DerefMut for Response {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response
    }
}

/// RequestBuilder Adapter
pub struct RequestBuilder(ReqBuilder);

impl RequestBuilder {
    /// Creates a new, default `RequestBuilder`.
    pub fn new() -> Self {
        Self(ReqBuilder::new())
    }

    /// Sets the `Method` of the `Request`.
    pub fn method<T>(self, method: T) -> Self
    where
        Method: TryFrom<T>,
        <Method as TryFrom<T>>::Error: Into<HttpError>,
    {
        Self(self.0.method(method))
    }

    /// Sets the `Uri` of the `Request`. `Uri` does not provide a default value,
    /// so it must be set.
    pub fn url<T>(self, uri: T) -> Self
    where
        Uri: TryFrom<T>,
        <Uri as TryFrom<T>>::Error: Into<HttpError>,
    {
        Self(self.0.url(uri))
    }

    /// Sets the `Version` of the `Request`. Uses `Version::HTTP11` by default.
    pub fn version<T>(mut self, version: T) -> Self
    where
        Version: TryFrom<T>,
        <Version as TryFrom<T>>::Error: Into<HttpError>,
    {
        self.0 = self.0.version(version);
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
        self.0 = self.0.header(name, value);
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
        self.0 = self.0.append_header(name, value);
        self
    }

    /// Try to create a `Request` based on the incoming `body`.
    pub fn body<T>(self, body: T) -> Result<Request<T>, HttpClientError> {
        self.0
            .body(body)
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Build, Some(e)))
    }

    /// Creates a `Request` that uses this `RequestBuilder` configuration and
    /// the provided `Multipart`. You can also provide a `Uploader<Multipart>`
    /// as the body.
    ///
    /// # Error
    ///
    /// This method fails if some configurations are wrong.
    pub fn multipart<T>(self, body: T) -> Result<Request<T>, HttpClientError>
    where
        T: AsRef<MultiPart>,
    {
        self.0
            .multipart(body)
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Build, Some(e)))
    }
}

impl Default for RequestBuilder {
    fn default() -> Self {
        Self::new()
    }
}
