// Copyright (c) 2024 Huawei Device Co., Ltd.
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

use std::io::Read;
use std::sync::Arc;

use ylong_http_client::async_impl::{Body, Client, ClientBuilder, Downloader, Request};
use ylong_http_client::{CertVerifier, HttpClientError, ServerCerts};

struct Verifier;

impl CertVerifier for Verifier {
    fn verify(&self, certs: &ServerCerts) -> bool {
        // get version
        let _ = certs.version().unwrap();
        // get issuer
        let _ = certs.issuer().unwrap();
        // get name
        let _ = certs.cert_name().unwrap();
        // cmp cert file
        let mut file = std::fs::File::open("./tests/file/cert.pem").unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        let _ = certs.cmp_pem_cert(contents.as_bytes()).unwrap();
        println!("Custom Verified");
        false
    }
}

fn main() -> Result<(), HttpClientError> {
    let mut handles = Vec::new();

    let client = Arc::new(
        ClientBuilder::new()
            .cert_verifier(Verifier)
            .build()
            .unwrap(),
    );

    for _ in 0..4 {
        let temp = client.clone();
        handles.push(ylong_runtime::spawn(request(temp)));
    }
    for handle in handles {
        let _ = ylong_runtime::block_on(handle);
    }
    Ok(())
}

async fn request(client: Arc<Client>) -> Result<(), HttpClientError> {
    let request = Request::builder()
        .url("https://www.example.com")
        .body(Body::empty())
        .unwrap();
    // Sends request and receives a `Response`.
    let response = client.request(request).await?;
    // Reads the body of `Response` by using `BodyReader`.
    Downloader::console(response).download().await
}
