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

//! This is a simple synchronous HTTPS client example.

use ylong_http_client::sync_impl::Client;
use ylong_http_client::{Certificate, HttpClientError, Redirect, Request, TlsVersion};

fn main() {
    let mut v = vec![];
    for _i in 0..3 {
        let handle = std::thread::spawn(|| req);
        v.push(handle);
    }

    for h in v {
        let _ = h.join();
    }
}

fn req() -> Result<(), HttpClientError> {
    let v = "some certs".as_bytes();
    let cert = Certificate::from_pem(v)?;

    // Creates a `async_impl::Client`
    let client = Client::builder()
        .redirect(Redirect::default())
        .tls_built_in_root_certs(false) // not use root certs
        .danger_accept_invalid_certs(true) // not verify certs
        .max_tls_version(TlsVersion::TLS_1_2)
        .min_tls_version(TlsVersion::TLS_1_2)
        .add_root_certificate(cert)
        .build()?;

    // Creates a `Request`.
    let request = Request::get("https://www.baidu.com")
        .body("".as_bytes())
        .map_err(HttpClientError::other)?;

    // Sends request and receives a `Response`.
    let response = client.request(request)?;

    println!("{}", response.status().as_u16());
    println!("{}", response.headers());
    Ok(())
}
