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

#![cfg(all(
    feature = "async",
    feature = "http1_1",
    feature = "ylong_base",
    feature = "__tls"
))]

#[macro_use]
pub mod tcp_server;

use ylong_http_client::async_impl::Client;
use ylong_http_client::{Certificate, TlsVersion};

/// UT test cases for `Client::builder` function.
///
/// # Brief
/// 1. Calls `Client::builder`.
/// 2. Checks if the results are correct.
#[test]
fn sdv_client_tls_builder() {
    let client = Client::builder()
        .max_tls_version(TlsVersion::TLS_1_3)
        .min_tls_version(TlsVersion::TLS_1_0)
        .add_root_certificate(Certificate::from_pem(b"cert").unwrap())
        .tls_ca_file("ca.crt")
        .tls_cipher_list("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
        .tls_cipher_suites("DEFAULT:!aNULL:!eNULL:!MD5:!3DES:!DES:!RC4:!IDEA:!SEED:!aDSS:!SRP:!PSK")
        .tls_built_in_root_certs(false)
        .danger_accept_invalid_certs(false)
        .danger_accept_invalid_hostnames(false)
        .tls_sni(false)
        .build();

    assert!(client.is_err());
}
