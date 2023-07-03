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

use crate::async_impl::{Body, HttpBody, StreamData};
use crate::error::{ErrorKind, HttpClientError};
use crate::util::dispatcher::http1::Http1Conn;
use crate::util::normalizer::BodyLengthParser;
use crate::Request;
use crate::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use ylong_http::h1::{RequestEncoder, ResponseDecoder};
use ylong_http::response::Response;

const TEMP_BUF_SIZE: usize = 16 * 1024;

pub(crate) async fn request<S, T>(
    mut conn: Http1Conn<S>,
    request: &mut Request<T>,
) -> Result<Response<HttpBody>, HttpClientError>
where
    T: Body,
    S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
{
    let mut buf = vec![0u8; TEMP_BUF_SIZE];

    // Encodes and sends Request-line and Headers(non-body fields).
    let mut non_body = RequestEncoder::new(request.part().clone());
    non_body.set_proxy(true);
    loop {
        match non_body.encode(&mut buf[..]) {
            Ok(0) => break,
            Ok(written) => {
                // RequestEncoder writes `buf` as much as possible.
                if let Err(e) = conn.raw_mut().write_all(&buf[..written]).await {
                    conn.shutdown();
                    return Err(HttpClientError::new_with_cause(ErrorKind::Request, Some(e)));
                }
            }
            Err(e) => {
                conn.shutdown();
                return Err(HttpClientError::new_with_cause(ErrorKind::Request, Some(e)));
            }
        }
    }

    // Encodes Request Body.
    let body = request.body_mut();
    let mut written = 0;
    let mut end_body = false;
    while !end_body {
        if written < buf.len() {
            match body.data(&mut buf[written..]).await {
                Ok(0) => end_body = true,
                Ok(size) => written += size,
                Err(e) => {
                    conn.shutdown();
                    return Err(HttpClientError::new_with_cause(
                        ErrorKind::BodyTransfer,
                        Some(e),
                    ));
                }
            }
        }
        if written == buf.len() || end_body {
            if let Err(e) = conn.raw_mut().write_all(&buf[..written]).await {
                conn.shutdown();
                return Err(HttpClientError::new_with_cause(
                    ErrorKind::BodyTransfer,
                    Some(e),
                ));
            }
            written = 0;
        }
    }

    // Decodes response part.
    let (part, pre) = {
        let mut decoder = ResponseDecoder::new();
        loop {
            let size = match conn.raw_mut().read(buf.as_mut_slice()).await {
                Ok(0) => {
                    conn.shutdown();
                    return Err(HttpClientError::new_with_message(
                        ErrorKind::Request,
                        "Tcp Closed",
                    ));
                }
                Ok(size) => size,
                Err(e) => {
                    conn.shutdown();
                    return Err(HttpClientError::new_with_cause(ErrorKind::Request, Some(e)));
                }
            };

            match decoder.decode(&buf[..size]) {
                Ok(None) => {}
                Ok(Some((part, rem))) => break (part, rem),
                Err(e) => {
                    conn.shutdown();
                    return Err(HttpClientError::new_with_cause(ErrorKind::Request, Some(e)));
                }
            }
        }
    };

    let length = match BodyLengthParser::new(request.method(), &part).parse() {
        Ok(length) => length,
        Err(e) => {
            conn.shutdown();
            return Err(e);
        }
    };

    let body = HttpBody::new(length, Box::new(conn), pre)?;
    Ok(Response::from_raw_parts(part, body))
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
