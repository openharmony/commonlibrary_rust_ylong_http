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

use ylong_http_client::async_impl::{Client, Downloader};
use ylong_http_client::util::Redirect;
use ylong_http_client::{Certificate, Request, TlsVersion};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime build err.");
    let mut v = vec![];
    for _i in 0..1 {
        let handle = rt.spawn(req());
        v.push(handle);
    }

    rt.block_on(async move {
        for h in v {
            let _ = h.await;
        }
    });
}

async fn req() {
    let v = "some certs".as_bytes();
    let cert = Certificate::from_pem(v);
    // Creates a `async_impl::Client`
    let client = Client::builder()
        .redirect(Redirect::default())
        .tls_built_in_root_certs(true) // default true
        .max_tls_version(TlsVersion::TLS_1_2)
        .min_tls_version(TlsVersion::TLS_1_2)
        .add_root_certificate(cert.unwrap())
        .build()
        .unwrap();

    // Creates a `Request`.
    let request = Request::get(
        "https://www.baidu.com",
        // "https://app.prntscr.com/build/setup-lightshot.exe",
    )
    .body("".as_bytes())
    .unwrap();

    // Sends request and receives a `Response`.
    let response = client.request(request).await.unwrap();
    // assert!(response.is_ok());

    println!("{}", response.status().as_u16());
    println!("{}", response.headers());

    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(response).download().await;
}
