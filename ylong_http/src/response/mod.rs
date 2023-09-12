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

//! HTTP [`Response`].

pub mod status;
use status::StatusCode;

use crate::headers::Headers;
use crate::version::Version;

// TODO: `ResponseBuilder` implementation.

/// HTTP `Response` Implementation.
///
/// The status-line and field-line of a response-message are stored in
/// `Response`.
///
/// The body can be saved in the user-defined type.
///
/// According to the [`RFC9112`], the origin reason-phrase of status-line is not
/// saved, the unified reason in `StatusCode` is used to indicate the status
/// code reason.
///
/// [`RFC9112`]: https://httpwg.org/specs/rfc9112.html
/// [`Response`]: https://httpwg.org/specs/rfc9112.html#status.line
pub struct Response<T> {
    part: ResponsePart,
    body: T,
}

impl<T> Response<T> {
    /// Gets an immutable reference to the `Version`.
    pub fn version(&self) -> &Version {
        &self.part.version
    }

    /// Gets the `StatusCode`.
    pub fn status(&self) -> StatusCode {
        self.part.status
    }

    /// Gets an immutable reference to the `Headers`.
    pub fn headers(&self) -> &Headers {
        &self.part.headers
    }

    /// Gets an immutable reference to the `Body`.
    pub fn body(&self) -> &T {
        &self.body
    }

    /// Gets a mutable reference to the `Body`.
    pub fn body_mut(&mut self) -> &mut T {
        &mut self.body
    }

    /// Splits `Response` into `ResponsePart` and `Body`.
    pub fn into_parts(self) -> (ResponsePart, T) {
        (self.part, self.body)
    }

    /// Response construction method with parameters
    pub fn from_raw_parts(part: ResponsePart, body: T) -> Response<T> {
        Self { part, body }
    }
}

impl<T: Clone> Clone for Response<T> {
    fn clone(&self) -> Self {
        Self::from_raw_parts(self.part.clone(), self.body.clone())
    }
}

/// `ResponsePart`, which is called [`Status Line`] in [`RFC9112`].
///
/// A request-line begins with a method token, followed by a single space (SP),
/// the request-target, and another single space (SP), and ends with the
/// protocol version.
///
/// [`RFC9112`]: https://httpwg.org/specs/rfc9112.html
/// [`Status Line`]: https://httpwg.org/specs/rfc9112.html#status.line
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResponsePart {
    /// HTTP Version implementation.
    pub version: Version,
    /// HTTP Status Codes implementation.
    pub status: StatusCode,
    /// HTTP Headers, which is called Fields in RFC9110.
    pub headers: Headers,
}

#[cfg(test)]
#[cfg(feature = "http1_1")]
mod ut_response {
    use crate::h1::ResponseDecoder;
    use crate::headers::Headers;
    use crate::response::Response;

    const ERROR_HEADER: &str = "header append failed";

    /// UT test cases for `Response::version`.
    ///
    /// # Brief
    /// 1. Creates a `ResponsePart` by calling `ResponseDecoder::decode`.
    /// 2. Gets the reference of a `Version` by calling `Response::version`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_version() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response {
            part: result.0,
            body: result.1,
        };
        assert_eq!(response.version().as_str(), "HTTP/1.1")
    }

    /// UT test cases for `Response::status_code`.
    ///
    /// # Brief
    /// 1. Creates a `ResponsePart` by calling `ResponseDecoder::decode`.
    /// 2. Gets the reference of a `StatusCode` by calling
    ///    `Response::status_code`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_status_code() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response {
            part: result.0,
            body: result.1,
        };
        assert_eq!(response.status().as_u16(), 304)
    }

    /// UT test cases for `Response::headers`.
    ///
    /// # Brief
    /// 1. Creates a `ResponsePart` by calling `ResponseDecoder::decode`.
    /// 2. Gets the reference of a `Headers` by calling `Response::headers`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_headers() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response {
            part: result.0,
            body: result.1,
        };
        let mut headers = Headers::new();
        headers.insert("age", "270646").expect(ERROR_HEADER);
        headers
            .insert("Date", "Mon, 19 Dec 2022 01:46:59 GMT")
            .expect(ERROR_HEADER);
        headers
            .insert("Etag", "\"3147526947+gzip\"")
            .expect(ERROR_HEADER);
        assert_eq!(response.headers(), &headers)
    }

    /// UT test cases for `Response::body`.
    ///
    /// # Brief
    /// 1. Creates a body by calling `ResponseDecoder::decode`.
    /// 2. Gets the reference of a `&[u8]` body by calling `Response::body`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_body() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response {
            part: result.0,
            body: result.1,
        };
        assert_eq!(*response.body(), "body part".as_bytes())
    }

    /// UT test cases for `Response::body_mut`.
    ///
    /// # Brief
    /// 1. Creates a body by calling `ResponseDecoder::decode`.
    /// 2. Gets the reference of a `&[u8]` body by calling `Response::body_mut`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_body_mut() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let mut response = Response {
            part: result.0,
            body: result.1,
        };
        assert_eq!(*response.body_mut(), "body part".as_bytes())
    }

    /// UT test cases for `Response::into_parts`.
    ///
    /// # Brief
    /// 1. Creates a body by calling `ResponseDecoder::into_parts`.
    /// 2. Checks if the test result is correct.
    #[test]
    fn ut_response_into_parts() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response {
            part: result.0,
            body: result.1,
        };
        let (part, body) = response.into_parts();
        assert!(body.is_empty());
        assert_eq!(part.version.as_str(), "HTTP/1.1");
    }

    /// UT test cases for `Response::from_raw_parts`.
    ///
    /// # Brief
    /// 1. Creates a body and a part by calling `ResponseDecoder::decode`.
    /// 2. Creates a `Response` by calling `Response::from_raw_parts`.
    /// 3. Creates a `Response` by calling `Response::clone`.
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_response_from_raw_parts() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\nbody part".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response::<&[u8]>::from_raw_parts(result.0, result.1);
        assert_eq!(response.version().as_str(), "HTTP/1.1");
        assert_eq!(response.status().as_u16(), 304);
        let mut headers = Headers::new();
        headers.insert("age", "270646").expect(ERROR_HEADER);
        headers
            .insert("Date", "Mon, 19 Dec 2022 01:46:59 GMT")
            .expect(ERROR_HEADER);
        headers
            .insert("Etag", "\"3147526947+gzip\"")
            .expect(ERROR_HEADER);
        assert_eq!(response.headers(), &headers);
        assert_eq!(*response.body(), "body part".as_bytes())
    }
}
