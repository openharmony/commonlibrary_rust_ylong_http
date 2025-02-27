// Copyright (c) 2025 Huawei Device Co., Ltd.
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

use ylong_http_client::async_impl::{Body, Client, DefaultDnsResolver, Downloader, Request};
use ylong_http_client::HttpClientError;

fn main() -> Result<(), HttpClientError> {
    let handle = ylong_runtime::spawn(async move {
        connect().await.unwrap();
    });

    let _ = ylong_runtime::block_on(handle);
    Ok(())
}

async fn connect() -> Result<(), HttpClientError> {
    // Creates a `Default Dns Resolver`.
    let default_dns_resolver = DefaultDnsResolver::new();

    // Creates a `async_impl::Client`
    let default_dns_client = Client::builder()
        .dns_resolver(default_dns_resolver)
        .build()
        .unwrap();

    let default_dns_response = default_dns_client
        .request(
            Request::builder()
                .url("https://www.example.com")
                .body(Body::empty())?,
        )
        .await?;

    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(default_dns_response).download().await;

    Ok(())
}
