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

use std::pin::Pin;
use std::task::{Context, Poll};

use ylong_http::body::async_impl::Body;
use ylong_http::body::{ChunkBody, TextBody};
use ylong_http::h1::{RequestEncoder, ResponseDecoder};
use ylong_http::request::uri::Scheme;
use ylong_http::version::Version;

use super::StreamData;
use crate::async_impl::connector::ConnInfo;
use crate::async_impl::request::Message;
use crate::async_impl::{HttpBody, Response};
use crate::error::HttpClientError;
use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use crate::util::dispatcher::http1::Http1Conn;
use crate::util::normalizer::BodyLengthParser;

const TEMP_BUF_SIZE: usize = 16 * 1024;

pub(crate) async fn request<S>(
    mut conn: Http1Conn<S>,
    mut message: Message,
) -> Result<Response, HttpClientError>
where
    S: AsyncRead + AsyncWrite + ConnInfo + Sync + Send + Unpin + 'static,
{
    message
        .interceptor
        .intercept_connection(conn.raw_mut().conn_detail())?;
    message
        .interceptor
        .intercept_request(message.request.ref_mut())?;
    let mut buf = vec![0u8; TEMP_BUF_SIZE];

    // Encodes and sends Request-line and Headers(non-body fields).
    let mut part_encoder = RequestEncoder::new(message.request.ref_mut().part().clone());
    if conn.raw_mut().is_proxy() && message.request.ref_mut().uri().scheme() == Some(&Scheme::HTTP)
    {
        part_encoder.absolute_uri(true);
    }
    loop {
        match part_encoder.encode(&mut buf[..]) {
            Ok(0) => break,
            Ok(written) => {
                message.interceptor.intercept_input(&buf[..written])?;
                // RequestEncoder writes `buf` as much as possible.
                if let Err(e) = conn.raw_mut().write_all(&buf[..written]).await {
                    conn.shutdown();
                    return err_from_io!(Request, e);
                }
            }
            Err(e) => {
                conn.shutdown();
                return err_from_other!(Request, e);
            }
        }
    }

    let content_length = message
        .request
        .ref_mut()
        .part()
        .headers
        .get("Content-Length")
        .and_then(|v| v.to_string().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some();

    let transfer_encoding = message
        .request
        .ref_mut()
        .part()
        .headers
        .get("Transfer-Encoding")
        .and_then(|v| v.to_string().ok())
        .map(|v| v.contains("chunked"))
        .unwrap_or(false);

    let body = message.request.ref_mut().body_mut();

    match (content_length, transfer_encoding) {
        (_, true) => {
            let body = ChunkBody::from_async_reader(body);
            encode_body(&mut conn, body, &mut buf).await?;
        }
        (true, false) => {
            let body = TextBody::from_async_reader(body);
            encode_body(&mut conn, body, &mut buf).await?;
        }
        (false, false) => {
            let body = TextBody::from_async_reader(body);
            encode_body(&mut conn, body, &mut buf).await?;
        }
    };

    // Decodes response part.
    let (part, pre) = {
        let mut decoder = ResponseDecoder::new();
        loop {
            let size = match conn.raw_mut().read(buf.as_mut_slice()).await {
                Ok(0) => {
                    conn.shutdown();
                    return err_from_msg!(Request, "Tcp closed");
                }
                Ok(size) => size,
                Err(e) => {
                    conn.shutdown();
                    return err_from_io!(Request, e);
                }
            };

            message.interceptor.intercept_output(&buf[..size])?;
            match decoder.decode(&buf[..size]) {
                Ok(None) => {}
                Ok(Some((part, rem))) => break (part, rem),
                Err(e) => {
                    conn.shutdown();
                    return err_from_other!(Request, e);
                }
            }
        }
    };

    // The shutdown function only sets the current connection to the closed state
    // and does not release the connection immediately.
    // Instead, the connection will be completely closed
    // when the body has finished reading or when the body is released.
    match part.headers.get("Connection") {
        None => {
            if part.version == Version::HTTP1_0 {
                conn.shutdown()
            }
        }
        Some(value) => {
            if part.version == Version::HTTP1_0 {
                if value
                    .to_string()
                    .ok()
                    .and_then(|v| v.find("keep-alive"))
                    .is_none()
                {
                    conn.shutdown()
                }
            } else if value
                .to_string()
                .ok()
                .and_then(|v| v.find("close"))
                .is_some()
            {
                conn.shutdown()
            }
        }
    }

    let length = match BodyLengthParser::new(message.request.ref_mut().method(), &part).parse() {
        Ok(length) => length,
        Err(e) => {
            conn.shutdown();
            return Err(e);
        }
    };

    let body = HttpBody::new(message.interceptor, length, Box::new(conn), pre)?;
    Ok(Response::new(
        ylong_http::response::Response::from_raw_parts(part, body),
    ))
}

async fn encode_body<S, T>(
    conn: &mut Http1Conn<S>,
    mut body: T,
    buf: &mut [u8],
) -> Result<(), HttpClientError>
where
    T: Body,
    S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
{
    // Encodes Request Body.
    let mut written = 0;
    let mut end_body = false;
    while !end_body {
        if written < buf.len() {
            match body.data(&mut buf[written..]).await {
                Ok(0) => end_body = true,
                Ok(size) => written += size,
                Err(e) => {
                    conn.shutdown();

                    let error = e.into();
                    // When using `Uploader`, here we can get `UserAborted` error.
                    return if error.source().is_some() {
                        Err(HttpClientError::user_aborted())
                    } else {
                        err_from_other!(BodyTransfer, error)
                    };
                }
            }
        }
        if written == buf.len() || end_body {
            if let Err(e) = conn.raw_mut().write_all(&buf[..written]).await {
                conn.shutdown();
                return err_from_io!(BodyTransfer, e);
            }
            written = 0;
        }
    }
    Ok(())
}

impl<S: AsyncRead + Unpin> AsyncRead for Http1Conn<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(self.raw_mut()).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + Unpin> StreamData for Http1Conn<S> {
    fn shutdown(&self) {
        Self::shutdown(self)
    }
}
