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

//! This is a simple synchronous HTTP client example using the ylong_http_client
//! crate. It demonstrates creating a client, making a request, and reading the
//! response.

use ylong_http_client::sync_impl::{BodyReader, Client};
use ylong_http_client::{HttpClientError, Request};

fn main() -> Result<(), HttpClientError> {
    // Creates a `sync_impl::Client`
    let client = Client::new();

    // Creates a `Request`.
    let request = Request::get("127.0.0.1:3000")
        .body("".as_bytes())
        .map_err(|e| HttpClientError::other(Some(e)))?;

    // Sends request and receives a `Response`.
    let mut response = client.request(request)?;

    // Reads the body of `Response` by using `BodyReader`.
    let _ = BodyReader::default().read_all(response.body_mut());
    Ok(())
}
