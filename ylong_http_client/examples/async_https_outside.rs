/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use ylong_http_client::async_impl::{Client, Downloader};
use ylong_http_client::util::Redirect;
use ylong_http_client::{Certificate, Request};

#[tokio::main]
async fn main() {
    let v = include_bytes!("./test.pem");
    let cert = Certificate::from_pem(v);
    // Creates a `async_impl::Client`
    let client = Client::builder()
        .redirect(Redirect::default())
        .add_root_certificate(cert.unwrap())
        .build()
        .unwrap();

    // Creates a `Request`.
    let request = Request::get("https://www.huawei.com")
        .body("".as_bytes())
        .unwrap();

    // Sends request and receives a `Response`.
    let response = client.request(request).await;
    assert!(response.is_ok());

    // Reads the body of `Response` by using `BodyReader`.
    let _ = Downloader::console(response.unwrap()).download().await;
}
