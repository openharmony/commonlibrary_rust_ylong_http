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

//! This is a simple asynchronous HTTPS client example.

use ylong_http_client::async_impl::{Body, Client, Downloader, Request};
use ylong_http_client::{HttpClientError, Redirect, TlsVersion};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime build err.");
    let handle = rt.spawn(req());

    rt.block_on(async move {
        let _ = handle.await.unwrap().unwrap();
    });
}

async fn req() -> Result<(), HttpClientError> {
    // Creates a `async_impl::Client`
    let client = Client::builder()
        .redirect(Redirect::default())
        .min_tls_version(TlsVersion::TLS_1_2)
        .build()?;

    // Creates a `Request`.
    let request = Request::builder()
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0")
        .url("http://vipspeedtest8.wuhan.net.cn:8080/download?size=1073741824")
        .body(Body::empty())?;

    // Sends request and receives a `Response`.
    let response = client.request(request).await?;

    println!("{}", response.status().as_u16());
    println!("{}", response.headers());

    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(response).download().await;
    Ok(())
}
