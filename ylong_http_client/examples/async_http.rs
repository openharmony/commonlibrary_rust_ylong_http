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

use ylong_http_client::async_impl::{Body, Client, Downloader, Request};
use ylong_http_client::HttpClientError;

fn main() -> Result<(), HttpClientError> {
    let handle = ylong_runtime::spawn(async move {
        client_send().await.unwrap();
    });

    let _ = ylong_runtime::block_on(handle);
    Ok(())
}

async fn client_send() -> Result<(), HttpClientError> {
    // Creates a `async_impl::Client`
    let client = Client::new();

    // Creates a `Request`.
    let request = Request::builder()
        .url("https://www.example.com")
        .body(Body::empty())?;

    // Sends request and receives a `Response`.
    let response = client.request(request).await?;

    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(response).download().await;
    Ok(())
}
