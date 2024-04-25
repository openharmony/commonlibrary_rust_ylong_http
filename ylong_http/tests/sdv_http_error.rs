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

#![cfg(all(feature = "http1_1", feature = "ylong_base"))]

use ylong_http::body::EmptyBody;
use ylong_http::h1::ResponseDecoder;
use ylong_http::request::Request;

/// SDV test cases for `HttpError`.
///
/// # Brief
/// 1. Creates a HttpError by build invalid url.
/// 2. check the error.
/// 3. Creates a HttpError by decode invalid response.
/// 4. check the error.
#[test]
fn sdv_client_send_request_repeatedly() {
    let request_err = Request::builder()
        .url("htttp:///path")
        .body(EmptyBody::new())
        .err();
    assert!(request_err.is_some());
    assert_eq!(
        format!("{:?}", request_err.unwrap()),
        "HttpError { kind: Uri(InvalidScheme) }"
    );

    // We need to create a decoder first.
    let mut decoder = ResponseDecoder::new();

    // Then we use it to decode some bytes.
    // The first part of bytes is correct, but we need more bytes to get a
    // `ResponsePart`.
    assert_eq!(
        format!(
            "{:?}",
            decoder.decode(b"HTTP/1.3 304 OK\r\nCon").err().unwrap()
        ),
        "HttpError { kind: H1(InvalidResponse) }"
    );
    assert_eq!(
        format!(
            "{}",
            decoder.decode(b"HTTP/1.3 304 OK\r\nCon").err().unwrap()
        ),
        "HttpError { kind: H1(InvalidResponse) }"
    );
}
