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

//! `ylong_http_client` `Request` adapter.

use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io::Cursor;
use std::sync::Arc;

use ylong_http::body::MultiPartBase;
use ylong_http::request::uri::PercentEncoder as PerEncoder;
use ylong_http::request::{Request as Req, RequestBuilder as ReqBuilder};

use crate::async_impl::interceptor::Interceptors;
use crate::error::{ErrorKind, HttpClientError};
use crate::runtime::{AsyncRead, ReadBuf};
use crate::util::request::RequestArc;

/// A structure that represents an HTTP `Request`. It contains a request line,
/// some HTTP headers and a HTTP body.
///
/// An HTTP request is made by a client, to a named host, which is located on a
/// server. The aim of the request is to access a resource on the server.
///
/// This structure is based on `ylong_http::Request<T>`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::{Body, Request};
///
/// let request = Request::builder().body(Body::empty());
/// ```
pub struct Request {
    pub(crate) inner: Req<Body>,
}

impl Request {
    /// Creates a new, default `RequestBuilder`.
    ///
    /// # Default
    ///
    /// - The method of this `RequestBuilder` is `GET`.
    /// - The URL of this `RequestBuilder` is `/`.
    /// - The HTTP version of this `RequestBuilder` is `HTTP/1.1`.
    /// - No headers are in this `RequestBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Request;
    ///
    /// let builder = Request::builder();
    /// ```
    pub fn builder() -> RequestBuilder {
        RequestBuilder::new()
    }
}

impl Deref for Request {
    type Target = Req<Body>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Request {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// A structure which used to build an HTTP `Request`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::{Body, RequestBuilder};
///
/// let request = RequestBuilder::new()
///     .method("GET")
///     .version("HTTP/1.1")
///     .url("http://www.example.com")
///     .header("Content-Type", "application/octet-stream")
///     .body(Body::empty());
/// ```
#[derive(Default)]
pub struct RequestBuilder(ReqBuilder);

impl RequestBuilder {
    /// Creates a new, default `RequestBuilder`.
    ///
    /// # Default
    ///
    /// - The method of this `RequestBuilder` is `GET`.
    /// - The URL of this `RequestBuilder` is `/`.
    /// - The HTTP version of this `RequestBuilder` is `HTTP/1.1`.
    /// - No headers are in this `RequestBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self(ReqBuilder::new())
    }

    /// Sets the `Method` of the `Request`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().method("GET");
    /// ```
    pub fn method(self, method: &str) -> Self {
        Self(self.0.method(method))
    }

    /// Sets the `Url` of the `Request`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().url("www.example.com");
    /// ```
    pub fn url(self, url: &str) -> Self {
        Self(self.0.url(url))
    }

    /// Sets the `Version` of the `Request`. Uses `Version::HTTP11` by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().version("HTTP/1.1");
    /// ```
    pub fn version(mut self, version: &str) -> Self {
        self.0 = self.0.version(version);
        self
    }

    /// Adds a `Header` to `Request`. Overwrites `HeaderValue` if the
    /// `HeaderName` already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().header("Content-Type", "application/octet-stream");
    /// ```
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.0 = self.0.header(name, value);
        self
    }

    /// Adds a `Header` to `Request`. Appends `HeaderValue` to the end of
    /// previous `HeaderValue` if the `HeaderName` already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::RequestBuilder;
    ///
    /// let builder = RequestBuilder::new().append_header("Content-Type", "application/octet-stream");
    /// ```
    pub fn append_header(mut self, name: &str, value: &str) -> Self {
        self.0 = self.0.append_header(name, value);
        self
    }

    /// Tries to create a `Request` based on the incoming `body`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::{Body, RequestBuilder};
    ///
    /// let request = RequestBuilder::new().body(Body::empty());
    /// ```
    pub fn body(self, body: Body) -> Result<Request, HttpClientError> {
        let mut builder = self;
        match body.inner {
            BodyKind::Slice(ref slice) => {
                builder = builder.header(
                    "Content-Length",
                    format!("{}", slice.get_ref().len()).as_str(),
                );
            }
            BodyKind::Multipart(ref multipart) => {
                let value = format!(
                    "multipart/form-data; boundary={}",
                    multipart.multipart().boundary()
                );

                builder = builder.header("Content-Type", value.as_str());

                if let Some(size) = multipart.multipart().total_bytes() {
                    builder = builder.header("Content-Length", format!("{size}").as_str());
                }
            }
            _ => {}
        }

        builder
            .0
            .body(body)
            .map(|inner| Request { inner })
            .map_err(|e| HttpClientError::from_error(ErrorKind::Build, e))
    }
}

/// A structure that represents body of HTTP request.
///
/// There are many kinds of body supported:
///
/// - Empty: an empty body.
/// - Slice: a body whose content comes from a memory slice.
/// - Stream: a body whose content comes from a stream.
/// - Multipart: a body whose content can transfer into a `Multipart`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::Body;
///
/// let body = Body::empty();
/// ```
pub struct Body {
    inner: BodyKind,
}

pub(crate) enum BodyKind {
    Empty,
    Slice(Cursor<Vec<u8>>),
    Stream(Box<dyn AsyncRead + Send + Sync + Unpin>),
    Multipart(Box<dyn MultiPartBase + Send + Sync + Unpin>),
}

impl Body {
    /// Creates an empty HTTP body.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Body;
    ///
    /// let body = Body::empty();
    /// ```
    pub fn empty() -> Self {
        Body::new(BodyKind::Empty)
    }

    /// Creates an HTTP body that based on memory slice.
    ///
    /// This kind of body is **reusable**.
    ///
    /// # Example
    ///
    /// ```
    /// use ylong_http_client::async_impl::Body;
    ///
    /// let body = Body::slice("HelloWorld");
    /// ```
    pub fn slice<T>(slice: T) -> Self
    where
        T: Into<Vec<u8>>,
    {
        Body::new(BodyKind::Slice(Cursor::new(slice.into())))
    }

    /// Creates an HTTP body that based on an asynchronous stream.
    ///
    /// This kind of body is not **reusable**.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::Body;
    ///
    /// let body = Body::stream("HelloWorld".as_bytes());
    /// ```
    pub fn stream<T>(stream: T) -> Self
    where
        T: AsyncRead + Send + Sync + Unpin + 'static,
    {
        Body::new(BodyKind::Stream(
            Box::new(stream) as Box<dyn AsyncRead + Send + Sync + Unpin>
        ))
    }

    /// Creates an HTTP body that based on a structure which implements
    /// `MultiPartBase`.
    ///
    /// This kind of body is not **reusable**.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::{Body, MultiPart};
    ///
    /// async fn create_multipart_body(multipart: MultiPart) {
    ///     let body = Body::multipart(multipart);
    /// }
    /// ```
    pub fn multipart<T>(stream: T) -> Self
    where
        T: MultiPartBase + Send + Sync + Unpin + 'static,
    {
        Body::new(BodyKind::Multipart(
            Box::new(stream) as Box<dyn MultiPartBase + Send + Sync + Unpin>
        ))
    }
}

impl Body {
    pub(crate) fn new(inner: BodyKind) -> Self {
        Self { inner }
    }

    // TODO: Considers reusing unread stream ?
    pub(crate) fn reuse(&mut self) -> bool {
        match self.inner {
            BodyKind::Empty => true,
            BodyKind::Slice(ref mut slice) => {
                slice.set_position(0);
                true
            }
            _ => false,
        }
    }
}

impl AsyncRead for Body {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        match this.inner {
            BodyKind::Empty => Poll::Ready(Ok(())),
            BodyKind::Slice(ref mut slice) => {
                #[cfg(feature = "tokio_base")]
                return Pin::new(slice).poll_read(cx, buf);
                #[cfg(feature = "ylong_base")]
                return poll_read_cursor(slice, buf);
            }
            BodyKind::Stream(ref mut stream) => Pin::new(stream).poll_read(cx, buf),
            BodyKind::Multipart(ref mut multipart) => Pin::new(multipart).poll_read(cx, buf),
        }
    }
}

/// HTTP url percent encoding implementation.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::PercentEncoder;
///
/// let url = "https://www.example.com/data/测试文件.txt";
/// let encoded = PercentEncoder::encode(url).unwrap();
/// assert_eq!(
///     encoded,
///     "https://www.example.com/data/%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt"
/// );
/// ```
pub struct PercentEncoder;

impl PercentEncoder {
    /// Percent-coding entry.
    pub fn encode(url: &str) -> Result<String, HttpClientError> {
        PerEncoder::parse(url).map_err(|e| HttpClientError::from_error(ErrorKind::Other, e))
    }
}

pub(crate) struct Message {
    pub(crate) request: RequestArc,
    pub(crate) interceptor: Arc<Interceptors>,
}

#[cfg(feature = "ylong_base")]
fn poll_read_cursor(
    cursor: &mut Cursor<Vec<u8>>,
    buf: &mut ylong_runtime::io::ReadBuf<'_>,
) -> Poll<std::io::Result<()>> {
    let pos = cursor.position();
    let data = (*cursor).get_ref();

    if pos > data.len() as u64 {
        return Poll::Ready(Ok(()));
    }

    let start = pos as usize;
    let target = std::cmp::min(data.len() - start, buf.remaining());
    let end = start + target;
    buf.append(&(data.as_slice())[start..end]);
    cursor.set_position(end as u64);

    Poll::Ready(Ok(()))
}

#[cfg(test)]
mod ut_client_request {
    use crate::async_impl::{Body, PercentEncoder, RequestBuilder};

    /// UT test cases for `RequestBuilder::default`.
    ///
    /// # Brief
    /// 1. Creates a `RequestBuilder` by `RequestBuilder::default`.
    /// 2. Checks if result is correct.
    #[test]
    fn ut_client_request_builder_default() {
        let builder = RequestBuilder::default().append_header("name", "value");
        let request = builder.body(Body::empty());
        assert!(request.is_ok());

        let request = RequestBuilder::default()
            .append_header("name", "value")
            .url("http://")
            .body(Body::empty());
        assert!(request.is_err())
    }

    /// UT test cases for `RequestBuilder::body`.
    ///
    /// # Brief
    /// 1. Creates a `RequestBuilder` by `RequestBuilder::body`.
    /// 2. Checks if result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_client_request_builder_body() {
        use std::pin::Pin;

        use ylong_http::body::{MultiPart, Part};
        use ylong_runtime::futures::poll_fn;
        use ylong_runtime::io::ReadBuf;

        use crate::runtime::AsyncRead;

        let mp = MultiPart::new().part(
            Part::new()
                .name("name")
                .file_name("example.txt")
                .mime("application/octet-stream")
                .stream("1234".as_bytes())
                .length(Some(4)),
        );
        let mut request = RequestBuilder::default().body(Body::multipart(mp)).unwrap();
        assert!(!request.body_mut().reuse());
        let handle = ylong_runtime::spawn(async move {
            let mut buf = vec![0u8; 50];
            let mut v_size = vec![];
            let mut v_str = vec![];

            loop {
                let mut read_buf = ReadBuf::new(&mut buf);
                poll_fn(|cx| Pin::new(request.body_mut()).poll_read(cx, &mut read_buf))
                    .await
                    .unwrap();

                let len = read_buf.filled().len();
                if len == 0 {
                    break;
                }
                v_size.push(len);
                v_str.extend_from_slice(&buf[..len]);
            }
            assert_eq!(v_size, vec![50, 50, 50, 50, 50, 11]);
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    /// UT test cases for `PercentEncoder::encode`.
    ///
    /// # Brief
    /// 1. Creates a `PercentEncoder`.
    /// 2. Checks if result is correct.
    #[test]
    fn ut_client_percent_encoder_encode() {
        let url = "https://www.example.com/data/测试文件.txt";
        let encoded = PercentEncoder::encode(url).unwrap();
        assert_eq!(
            encoded,
            "https://www.example.com/data/%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt"
        );
    }
}
