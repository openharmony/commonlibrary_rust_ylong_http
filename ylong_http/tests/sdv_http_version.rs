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

use ylong_http::version::Version;

/// SDV test cases for `Version`.
///
/// # Brief
/// 1. Creates a Version by build try_from.
/// 2. check the version.
/// 3. Invoke the as_str method.
/// 4. check result.
#[test]
fn sdv_client_send_request_repeatedly() {
    assert_eq!(Version::HTTP1_0.as_str(), "HTTP/1.0");
    assert_eq!(Version::HTTP1_1.as_str(), "HTTP/1.1");
    assert_eq!(Version::HTTP2.as_str(), "HTTP/2.0");
    assert_eq!(Version::HTTP3.as_str(), "HTTP/3.0");
    assert_eq!(Version::try_from("HTTP/1.0").unwrap(), Version::HTTP1_0);
    assert_eq!(Version::try_from("HTTP/1.1").unwrap(), Version::HTTP1_1);
    assert_eq!(Version::try_from("HTTP/2.0").unwrap(), Version::HTTP2);
    assert_eq!(Version::try_from("HTTP/3.0").unwrap(), Version::HTTP3);
    assert!(Version::try_from("http/1.0").is_err());
    assert!(Version::try_from("http/1.1").is_err());
    assert!(Version::try_from("http/2.0").is_err());
    assert!(Version::try_from("http/3.0").is_err());
}
