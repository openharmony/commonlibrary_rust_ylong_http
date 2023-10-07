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

// TODO: Remove this file later.

use ylong_http::request::method::Method;
use ylong_http::request::uri::Scheme;
use ylong_http::request::Request;
use ylong_http::response::status::StatusCode;
use ylong_http::response::ResponsePart;

use crate::{ErrorKind, HttpClientError, Uri};

pub(crate) struct RequestFormatter<'a, T> {
    part: &'a mut Request<T>,
}

impl<'a, T> RequestFormatter<'a, T> {
    pub(crate) fn new(part: &'a mut Request<T>) -> Self {
        Self { part }
    }

    pub(crate) fn normalize(&mut self) -> Result<(), HttpClientError> {
        let uri_formatter = UriFormatter::new();
        uri_formatter.format(self.part.uri_mut())?;

        let host_value = self.part.uri().authority().unwrap().to_str();

        if self.part.headers_mut().get("Accept").is_none() {
            let _ = self.part.headers_mut().insert("Accept", "*/*");
        }

        if self.part.headers_mut().get("Host").is_none() {
            let _ = self
                .part
                .headers_mut()
                .insert("Host", host_value.as_bytes());
        }

        Ok(())
    }
}

pub(crate) struct UriFormatter;

impl UriFormatter {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn format(&self, uri: &mut Uri) -> Result<(), HttpClientError> {
        let host = match uri.host() {
            Some(host) => host.clone(),
            None => {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Request,
                    "No host in url",
                ))
            }
        };

        #[cfg(feature = "__tls")]
        let mut scheme = Scheme::HTTPS;

        #[cfg(not(feature = "__tls"))]
        let mut scheme = Scheme::HTTP;

        if let Some(req_scheme) = uri.scheme() {
            scheme = req_scheme.clone()
        };

        let port;

        if let Some(req_port) = uri.port().and_then(|port| port.as_u16().ok()) {
            port = req_port;
        } else {
            match scheme {
                Scheme::HTTPS => port = 443,
                Scheme::HTTP => port = 80,
            }
        }

        let mut new_uri = Uri::builder();
        new_uri = new_uri.scheme(scheme);
        new_uri = new_uri.authority(format!("{}:{}", host.as_str(), port).as_bytes());

        if let Some(path) = uri.path() {
            new_uri = new_uri.path(path.clone());
        }

        if let Some(query) = uri.query() {
            new_uri = new_uri.query(query.clone());
        }

        *uri = new_uri.build().map_err(|_| {
            HttpClientError::new_with_message(ErrorKind::Request, "Normalize url failed")
        })?;

        Ok(())
    }
}

pub(crate) struct BodyLengthParser<'a> {
    req_method: &'a Method,
    part: &'a ResponsePart,
}

impl<'a> BodyLengthParser<'a> {
    pub(crate) fn new(req_method: &'a Method, part: &'a ResponsePart) -> Self {
        Self { req_method, part }
    }

    pub(crate) fn parse(&self) -> Result<BodyLength, HttpClientError> {
        if self.part.status.is_informational()
            || self.part.status == StatusCode::NO_CONTENT
            || self.part.status == StatusCode::NOT_MODIFIED
        {
            return Ok(BodyLength::Empty);
        }

        if (self.req_method == &Method::CONNECT && self.part.status.is_successful())
            || self.req_method == &Method::HEAD
        {
            return Ok(BodyLength::Empty);
        }

        #[cfg(feature = "http1_1")]
        {
            let transfer_encoding = self.part.headers.get("Transfer-Encoding");

            if transfer_encoding.is_some() {
                let transfer_encoding_contains_chunk = transfer_encoding
                    .and_then(|v| v.to_str().ok())
                    .and_then(|str| str.find("chunked"))
                    .is_some();

                return if transfer_encoding_contains_chunk {
                    Ok(BodyLength::Chunk)
                } else {
                    Ok(BodyLength::UntilClose)
                };
            }
        }

        let content_length = self.part.headers.get("Content-Length");

        if content_length.is_some() {
            let content_length_valid = content_length
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok());

            return match content_length_valid {
                // If `content-length` is 0, the io stream cannot be read,
                // otherwise it will get stuck.
                Some(0) => Ok(BodyLength::Empty),
                Some(len) => Ok(BodyLength::Length(len)),
                None => Err(HttpClientError::new_with_message(
                    ErrorKind::Request,
                    "Invalid response content-length",
                )),
            };
        }

        Ok(BodyLength::UntilClose)
    }
}
#[derive(PartialEq, Debug)]
pub(crate) enum BodyLength {
    #[cfg(feature = "http1_1")]
    Chunk,
    Length(usize),
    Empty,
    UntilClose,
}

#[cfg(test)]
mod ut_normalizer {
    use ylong_http::h1::ResponseDecoder;
    use ylong_http::request::method::Method;
    use ylong_http::request::uri::{Uri, UriBuilder};
    use ylong_http::request::Request;
    use ylong_http::response::Response;

    use crate::normalizer::UriFormatter;
    use crate::util::normalizer::{BodyLength, BodyLengthParser, RequestFormatter};

    /// UT test cases for `UriFormatter::format`.
    ///
    /// # Brief
    /// 1. Creates a `UriFormatter`.
    /// 2. Calls `UriFormatter::format` with `Uri` to get the result.
    /// 3. Checks if the uri port result is correct.
    #[test]
    fn ut_uri_format() {
        let mut uri = UriBuilder::new()
            .scheme("http")
            .authority("example.com")
            .path("/foo")
            .query("a=1")
            .build()
            .unwrap();
        let uni = UriFormatter::new();
        let _ = uni.format(&mut uri);
        assert_eq!(uri.port().unwrap().as_str(), "80");
    }

    /// UT test cases for `RequestFormatter::normalize`.
    ///
    /// # Brief
    /// 1. Creates a `RequestFormatter`.
    /// 2. Calls `UriFormatter::normalize` to get the result.
    /// 3. Checks if the request's header result is correct.
    #[test]
    fn ut_request_format() {
        let mut request = Request::new("this is a body");
        let request_uri = request.uri_mut();
        *request_uri = Uri::from_bytes(b"http://example1.com").unwrap();
        let mut formater = RequestFormatter::new(&mut request);
        let _ = formater.normalize();
        let (part, _) = request.into_parts();
        let res = part.headers.get("Host").unwrap();
        assert_eq!(res.to_str().unwrap().as_bytes(), b"example1.com:80");
    }

    /// UT test cases for `BodyLengthParser::parse`.
    ///
    /// # Brief
    /// 1. Creates a `BodyLengthParser`.
    /// 2. Calls `BodyLengthParser::parse` to get the result.
    /// 3. Checks if the BodyLength result is correct.
    #[test]
    fn ut_body_length_parser() {
        let response_str = "HTTP/1.1 202 \r\nAge: \t 270646 \t \t\r\nLocation: \t example3.com:80 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let method = Method::GET;
        let body_len_parser = BodyLengthParser::new(&method, &result.0);
        let res = body_len_parser.parse().unwrap();
        assert_eq!(res, BodyLength::UntilClose);

        let response_str = "HTTP/1.1 202 \r\nTransfer-Encoding: \t chunked \t \t\r\nLocation: \t example3.com:80 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let method = Method::GET;
        let body_len_parser = BodyLengthParser::new(&method, &result.0);
        let res = body_len_parser.parse().unwrap();
        assert_eq!(res, BodyLength::Chunk);

        let response_str = "HTTP/1.1 202 \r\nContent-Length: \t 20 \t \t\r\nLocation: \t example3.com:80 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let method = Method::GET;
        let body_len_parser = BodyLengthParser::new(&method, &result.0);
        let res = body_len_parser.parse().unwrap();
        assert_eq!(res, BodyLength::Length(20));
    }
}
