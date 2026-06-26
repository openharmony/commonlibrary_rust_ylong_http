// Copyright (c) 2026 Huawei Device Co., Ltd.
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

//! Sequential benchmark helper for HTTP requests sent through an HTTPS proxy.
//!
//! Configuration is provided with environment variables:
//! - `BENCH_URL`: target URL, for example `http://127.0.0.1:3000/data`.
//! - `BENCH_REQUESTS`: request count, default `100`.
//! - `PROXY_URL` or `HTTPS_PROXY`: proxy URL, for example `https://127.0.0.1:8443`.
//! - `PROXY_CA_FILE`: optional CA file used to verify the proxy certificate.
//! - `PROXY_CERT_FILE` and `PROXY_KEY_FILE`: optional proxy mTLS identity.
//! - `PROXY_KEY_TYPE`: `PEM` or `ASN1`, default `PEM`.
//! - `PROXY_INSECURE`: `1`/`true` to disable proxy cert and hostname verification.

use std::env;
use std::time::Instant;

use ylong_http::body::async_impl::Body as AsyncBody;
use ylong_http_client::async_impl::{Body, ClientBuilder, Request};
use ylong_http_client::{Proxy, TlsFileType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = env::var("BENCH_URL").unwrap_or_else(|_| "http://127.0.0.1:3000/".to_string());
    let requests = env::var("BENCH_REQUESTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);
    let proxy_url = env::var("PROXY_URL")
        .or_else(|_| env::var("HTTPS_PROXY"))
        .expect("set PROXY_URL or HTTPS_PROXY to an HTTPS proxy URL");
    let proxy_insecure = env_flag("PROXY_INSECURE");

    let mut proxy = Proxy::all(&proxy_url);
    if let Ok(ca_file) = env::var("PROXY_CA_FILE") {
        proxy = proxy.proxy_ca_file(&ca_file);
    }
    if let (Ok(cert_file), Ok(key_file)) = (env::var("PROXY_CERT_FILE"), env::var("PROXY_KEY_FILE"))
    {
        proxy = proxy.proxy_identity(&cert_file, &key_file, proxy_key_type());
    }
    if proxy_insecure {
        proxy = proxy
            .danger_accept_invalid_proxy_certs(true)
            .danger_accept_invalid_proxy_hostnames(true);
    }

    let client = ClientBuilder::new().proxy(proxy.build()?).build()?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut bytes = 0usize;

    let started = Instant::now();
    for _ in 0..requests {
        let request = Request::builder()
            .url(target.as_str())
            .body(Body::empty())?;
        let mut response = client.request(request).await?;
        loop {
            let read = response.body_mut().data(&mut buf).await?;
            if read == 0 {
                break;
            }
            bytes += read;
        }
    }
    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let req_per_sec = if elapsed_secs > 0.0 {
        requests as f64 / elapsed_secs
    } else {
        0.0
    };

    println!(
        "client=ylong requests={requests} bytes={bytes} elapsed_ms={} req_per_sec={req_per_sec:.2}",
        elapsed.as_millis()
    );
    Ok(())
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn proxy_key_type() -> TlsFileType {
    match env::var("PROXY_KEY_TYPE").ok().as_deref() {
        Some("ASN1") | Some("asn1") => TlsFileType::ASN1,
        _ => TlsFileType::PEM,
    }
}
