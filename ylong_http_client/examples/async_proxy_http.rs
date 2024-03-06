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

//! This is a simple asynchronous HTTP client example using the
//! ylong_http_client crate. It demonstrates creating a client, making a
//! request, and reading the response asynchronously.

use ylong_http_client::async_impl::{Body, ClientBuilder, Downloader, Request};
use ylong_http_client::{HttpClientError, Proxy};

#[tokio::main]
async fn main() -> Result<(), HttpClientError> {
    // Creates a `async_impl::Client`
    let client = ClientBuilder::new()
        .proxy(Proxy::all("http://proxy.example.com").build()?)
        .build()?;
    // Creates a `Request`.
    let request = Request::builder()
        .url("http://127.0.0.1:3000")
        .body(Body::empty())?;

    // Sends request and receives a `Response`.
    let response = client.request(request).await?;
    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(response).download().await;
    Ok(())
}
