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

#![cfg(all(
    feature = "async",
    feature = "http1_1",
    feature = "__tls",
    feature = "tokio_base"
))]

use core::pin::Pin;
use std::path::{Path, PathBuf};
use std::time::Duration;

use openssl::ssl::{Ssl, SslAcceptor, SslFiletype, SslMethod, SslVerifyMode};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_openssl::SslStream;
use ylong_http_client::async_impl::{Body as ClientBody, Client, Request};
use ylong_http_client::{Proxy, TlsFileType};

const CERT_FILE: &str = "tests/file/cert.pem";
const KEY_FILE: &str = "tests/file/key.pem";
const ROOT_CA_FILE: &str = "tests/file/root-ca.pem";

struct ProxyServer {
    port: u16,
    task: JoinHandle<()>,
}

#[test]
fn sdv_http_request_over_https_proxy() {
    let runtime = Runtime::new().expect("create tokio runtime failed");
    let proxy = start_https_proxy(&runtime, ProxyScenario::HttpForward);

    let root_ca = manifest_path(ROOT_CA_FILE);
    let proxy_url = format!("https://127.0.0.1:{}", proxy.port);
    let client = Client::builder()
        .proxy(
            Proxy::all(proxy_url.as_str())
                .proxy_ca_file(path_str(&root_ca))
                .danger_accept_invalid_proxy_hostnames(true)
                .build()
                .expect("build https proxy failed"),
        )
        .build()
        .expect("build client failed");

    runtime.block_on(async move {
        let request = Request::builder()
            .method("GET")
            .url("http://example.com/proxy-test")
            .body(ClientBody::empty())
            .expect("build request failed");
        let mut response = client.request(request).await.expect("send request failed");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(read_body(&mut response).await, b"http-over-https-proxy");
        proxy.task.await.expect("https proxy task failed");
    });
}

#[test]
fn sdv_https_request_over_https_proxy_with_proxy_mtls() {
    let runtime = Runtime::new().expect("create tokio runtime failed");
    let proxy = start_https_proxy(&runtime, ProxyScenario::HttpsTunnelWithMtls);

    let root_ca = manifest_path(ROOT_CA_FILE);
    let cert = manifest_path(CERT_FILE);
    let key = manifest_path(KEY_FILE);
    let proxy_url = format!("https://127.0.0.1:{}", proxy.port);
    let client = Client::builder()
        .tls_ca_file(path_str(&root_ca))
        .danger_accept_invalid_hostnames(true)
        .proxy(
            Proxy::all(proxy_url.as_str())
                .proxy_ca_file(path_str(&root_ca))
                .proxy_identity(path_str(&cert), path_str(&key), TlsFileType::PEM)
                .danger_accept_invalid_proxy_hostnames(true)
                .build()
                .expect("build https proxy failed"),
        )
        .build()
        .expect("build client failed");

    runtime.block_on(async move {
        let request = Request::builder()
            .method("GET")
            .url("https://127.0.0.1/secure")
            .body(ClientBody::empty())
            .expect("build request failed");
        let mut response = client.request(request).await.expect("send request failed");
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(read_body(&mut response).await, b"https-over-https-proxy");
        proxy.task.await.expect("https proxy task failed");
    });
}

#[test]
fn sdv_https_proxy_mtls_missing_client_cert_should_fail() {
    let runtime = Runtime::new().expect("create tokio runtime failed");
    let proxy = start_mtls_required_proxy_for_failure(&runtime);

    let root_ca = manifest_path(ROOT_CA_FILE);
    let proxy_url = format!("https://127.0.0.1:{}", proxy.port);
    let client = Client::builder()
        .proxy(
            Proxy::all(proxy_url.as_str())
                .proxy_ca_file(path_str(&root_ca))
                .danger_accept_invalid_proxy_hostnames(true)
                .build()
                .expect("build https proxy failed"),
        )
        .build()
        .expect("build client failed");

    runtime.block_on(async move {
        let request = Request::builder()
            .method("GET")
            .url("http://example.com/mtls-missing")
            .body(ClientBody::empty())
            .expect("build request failed");
        let result = timeout(Duration::from_secs(5), client.request(request))
            .await
            .expect("request timed out");
        assert!(
            result.is_err(),
            "HTTPS proxy should reject clients without a required certificate"
        );
        proxy.task.await.expect("mTLS failure proxy task failed");
    });
}

#[test]
fn sdv_https_proxy_connect_407_should_fail() {
    let runtime = Runtime::new().expect("create tokio runtime failed");
    let proxy = start_https_proxy(&runtime, ProxyScenario::HttpsTunnelAuthRequired);

    let root_ca = manifest_path(ROOT_CA_FILE);
    let proxy_url = format!("https://127.0.0.1:{}", proxy.port);
    let client = Client::builder()
        .proxy(
            Proxy::all(proxy_url.as_str())
                .proxy_ca_file(path_str(&root_ca))
                .danger_accept_invalid_proxy_hostnames(true)
                .build()
                .expect("build https proxy failed"),
        )
        .build()
        .expect("build client failed");

    runtime.block_on(async move {
        let request = Request::builder()
            .method("GET")
            .url("https://127.0.0.1/auth-required")
            .body(ClientBody::empty())
            .expect("build request failed");
        let result = timeout(Duration::from_secs(5), client.request(request))
            .await
            .expect("request timed out");
        assert!(
            result.is_err(),
            "CONNECT 407 from HTTPS proxy should fail the request"
        );
        proxy.task.await.expect("CONNECT 407 proxy task failed");
    });
}

fn start_https_proxy(runtime: &Runtime, scenario: ProxyScenario) -> ProxyServer {
    let listener = runtime
        .block_on(async { TcpListener::bind("127.0.0.1:0").await })
        .expect("bind https proxy failed");
    let port = listener
        .local_addr()
        .expect("get proxy local address failed")
        .port();

    let task = runtime.spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept proxy tcp failed");
        let mut proxy_tls = accept_tls(tcp, scenario.requires_proxy_client_cert()).await;
        if scenario.requires_proxy_client_cert() {
            assert!(
                proxy_tls.ssl().peer_certificate().is_some(),
                "proxy mTLS did not receive a client certificate"
            );
        }

        match scenario {
            ProxyScenario::HttpForward => handle_http_forward(&mut proxy_tls).await,
            ProxyScenario::HttpsTunnelWithMtls => handle_https_tunnel(proxy_tls).await,
            ProxyScenario::HttpsTunnelAuthRequired => {
                handle_connect_auth_required(&mut proxy_tls).await
            }
        }
    });

    ProxyServer { port, task }
}

fn start_mtls_required_proxy_for_failure(runtime: &Runtime) -> ProxyServer {
    let listener = runtime
        .block_on(async { TcpListener::bind("127.0.0.1:0").await })
        .expect("bind https proxy failed");
    let port = listener
        .local_addr()
        .expect("get proxy local address failed")
        .port();

    let task = runtime.spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept proxy tcp failed");
        let acceptor = build_acceptor(true);
        let ssl = Ssl::new(acceptor.context()).expect("create proxy ssl failed");
        let mut stream = SslStream::new(ssl, tcp).expect("create proxy ssl stream failed");
        let result = Pin::new(&mut stream).accept().await;
        assert!(
            result.is_err(),
            "proxy TLS unexpectedly accepted a client without certificate"
        );
    });

    ProxyServer { port, task }
}

async fn handle_http_forward<S>(stream: &mut S)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let req = read_headers(stream).await;
    assert!(
        req.starts_with("GET http://example.com:80/proxy-test HTTP/1.1\r\n"),
        "unexpected HTTP-over-HTTPS-proxy request line: {req:?}"
    );
    assert!(
        req.to_ascii_lowercase().contains("host:example.com\r\n"),
        "HTTP-over-HTTPS-proxy request should preserve origin host: {req:?}"
    );
    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 21\r\n\r\nhttp-over-https-proxy")
        .await
        .expect("write proxy response failed");
}

async fn handle_https_tunnel(mut proxy_tls: SslStream<TcpStream>) {
    let connect_req = read_headers(&mut proxy_tls).await;
    assert!(
        connect_req.starts_with("CONNECT 127.0.0.1:443 HTTP/1.1\r\n"),
        "unexpected CONNECT request line: {connect_req:?}"
    );
    assert!(
        connect_req
            .to_ascii_lowercase()
            .contains("host: 127.0.0.1:443\r\n"),
        "CONNECT request should contain target Host header: {connect_req:?}"
    );
    proxy_tls
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .expect("write CONNECT response failed");

    let mut origin_tls = accept_nested_origin_tls(proxy_tls).await;
    let req = read_headers(&mut origin_tls).await;
    assert!(
        req.starts_with("GET /secure HTTP/1.1\r\n"),
        "unexpected request sent inside CONNECT tunnel: {req:?}"
    );
    origin_tls
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 22\r\n\r\nhttps-over-https-proxy")
        .await
        .expect("write origin response failed");
}

async fn handle_connect_auth_required<S>(stream: &mut S)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let connect_req = read_headers(stream).await;
    assert!(
        connect_req.starts_with("CONNECT 127.0.0.1:443 HTTP/1.1\r\n"),
        "unexpected CONNECT request line for 407 case: {connect_req:?}"
    );
    stream
        .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")
        .await
        .expect("write CONNECT 407 response failed");
}

async fn accept_tls(tcp: TcpStream, require_client_cert: bool) -> SslStream<TcpStream> {
    let acceptor = build_acceptor(require_client_cert);
    let ssl = Ssl::new(acceptor.context()).expect("create proxy ssl failed");
    let mut stream = SslStream::new(ssl, tcp).expect("create proxy ssl stream failed");
    Pin::new(&mut stream)
        .accept()
        .await
        .expect("accept proxy tls failed");
    stream
}

async fn accept_nested_origin_tls(
    proxy_tls: SslStream<TcpStream>,
) -> SslStream<SslStream<TcpStream>> {
    let acceptor = build_acceptor(false);
    let ssl = Ssl::new(acceptor.context()).expect("create origin ssl failed");
    let mut stream = SslStream::new(ssl, proxy_tls).expect("create origin ssl stream failed");
    Pin::new(&mut stream)
        .accept()
        .await
        .expect("accept nested origin tls failed");
    stream
}

fn build_acceptor(require_client_cert: bool) -> SslAcceptor {
    let cert = manifest_path(CERT_FILE);
    let key = manifest_path(KEY_FILE);
    let root_ca = manifest_path(ROOT_CA_FILE);

    let mut builder =
        SslAcceptor::mozilla_intermediate(SslMethod::tls()).expect("create ssl acceptor failed");
    builder
        .set_session_id_context(b"https-proxy-test")
        .expect("set session id failed");
    builder
        .set_private_key_file(&key, SslFiletype::PEM)
        .expect("set private key failed");
    builder
        .set_certificate_chain_file(&cert)
        .expect("set certificate failed");
    builder
        .set_alpn_protos(b"\x08http/1.1")
        .expect("set alpn failed");
    builder.set_alpn_select_callback(|_, client| {
        openssl::ssl::select_next_proto(b"\x08http/1.1", client)
            .ok_or(openssl::ssl::AlpnError::NOACK)
    });

    if require_client_cert {
        builder
            .set_ca_file(&root_ca)
            .expect("set proxy client-cert ca failed");
        builder.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
    }

    builder.build()
}

async fn read_headers<S>(stream: &mut S) -> String
where
    S: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let read = stream.read(&mut tmp).await.expect("read request failed");
        assert!(read > 0, "unexpected EOF while reading request headers");
        buf.extend_from_slice(&tmp[..read]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        assert!(buf.len() <= 8192, "request headers too long");
    }
    String::from_utf8(buf).expect("request is not utf-8")
}

async fn read_body(response: &mut ylong_http_client::async_impl::Response) -> Vec<u8> {
    let mut body = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let read = response
            .data(&mut buf)
            .await
            .expect("read response body failed");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read]);
    }
    body
}

fn manifest_path(path: &str) -> PathBuf {
    let mut full = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    full.push(path);
    full
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("test path is not utf-8")
}

#[derive(Clone, Copy)]
enum ProxyScenario {
    HttpForward,
    HttpsTunnelWithMtls,
    HttpsTunnelAuthRequired,
}

impl ProxyScenario {
    fn requires_proxy_client_cert(self) -> bool {
        matches!(self, Self::HttpsTunnelWithMtls)
    }
}
