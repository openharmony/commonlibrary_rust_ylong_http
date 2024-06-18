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

use ylong_http::headers::{Header, HeaderName, HeaderValue, Headers};

/// SDV test cases for `Headers`.
///
/// # Brief
/// 1. Creates a HeaderName and a HeaderValue.
/// 2. check the HeaderName and HeaderValue.
/// 3. Creates a Header.
/// 4. check the error.
#[test]
fn sdv_client_send_request_repeatedly() {
    let name = HeaderName::from_bytes(b"John-Doe").unwrap();
    let value = HeaderValue::from_bytes(b"Foo").unwrap();
    let header = Header::from_raw_parts(name, value);
    let cloned_header = header.clone();
    assert_eq!(header, cloned_header);
    assert_eq!(format!("{:?}", header), "Header { name: HeaderName { name: \"john-doe\" }, value: HeaderValue { inner: [[70, 111, 111]], is_sensitive: false } }");

    assert_eq!(
        format!("{:?}", HeaderValue::from_bytes(b"Foo\r\n").err().unwrap()),
        "HttpError { kind: InvalidInput }"
    );
    let mut value = HeaderValue::from_bytes(b"Foo").unwrap();
    assert_eq!(
        format!("{:?}", value.append_bytes(b"Foo\r\n").err().unwrap()),
        "HttpError { kind: InvalidInput }"
    );

    let mut headers = Headers::new();
    headers.insert("key", "value").unwrap();
    assert_eq!(format!("{}", headers), "key: value\n");
    let mut_value = headers.get_mut("key");
    let mut value = HeaderValue::from_bytes(b"value").unwrap();
    assert_eq!(mut_value, Some(&mut value));
    assert_eq!(headers.remove("key"), Some(value));
}
