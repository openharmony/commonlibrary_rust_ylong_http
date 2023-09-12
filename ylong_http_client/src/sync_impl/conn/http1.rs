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

use std::io::{Read, Write};

use ylong_http::body::sync_impl::Body;
use ylong_http::h1::{RequestEncoder, ResponseDecoder};
use ylong_http::request::Request;
use ylong_http::response::Response;

use crate::error::{ErrorKind, HttpClientError};
use crate::sync_impl::conn::StreamData;
use crate::sync_impl::HttpBody;
use crate::util::dispatcher::http1::Http1Conn;

const TEMP_BUF_SIZE: usize = 16 * 1024;

pub(crate) fn request<S, T>(
    mut conn: Http1Conn<S>,
    request: &mut Request<T>,
) -> Result<Response<HttpBody>, HttpClientError>
where
    T: Body,
    S: Read + Write + 'static,
{
    let mut buf = vec![0u8; TEMP_BUF_SIZE];

    // Encodes request.
    let mut encode_part = Some(RequestEncoder::new(request.part().clone()));
    let mut encode_body = Some(request.body_mut());
    let mut write = 0;
    while encode_part.is_some() || encode_body.is_some() {
        if write < buf.len() {
            if let Some(part) = encode_part.as_mut() {
                let size = part
                    .encode(&mut buf[write..])
                    .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
                write += size;
                if size == 0 {
                    encode_part = None;
                }
            }
        }

        if write < buf.len() {
            if let Some(body) = encode_body.as_mut() {
                let size = body
                    .data(&mut buf[write..])
                    .map_err(|e| HttpClientError::from_error(ErrorKind::BodyTransfer, e))?;
                write += size;
                if size == 0 {
                    encode_body = None;
                }
            }
        }

        if write == buf.len() {
            conn.raw_mut()
                .write_all(&buf[..write])
                .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
            write = 0;
        }
    }

    if write != 0 {
        conn.raw_mut()
            .write_all(&buf[..write])
            .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
    }

    // Decodes response part.
    let (part, pre) = {
        let mut decoder = ResponseDecoder::new();
        loop {
            let size = conn
                .raw_mut()
                .read(buf.as_mut_slice())
                .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
            match decoder.decode(&buf[..size]) {
                Ok(None) => {}
                Ok(Some((part, rem))) => break (part, rem),
                Err(e) => return err_from_other!(Request, e),
            }
        }
    };

    // Generates response body.
    let body = {
        let chunked = part
            .headers
            .get("Transfer-Encoding")
            .map(|v| v.to_string().unwrap_or(String::new()))
            .and_then(|s| s.find("chunked"))
            .is_some();
        let content_length = part
            .headers
            .get("Content-Length")
            .map(|v| v.to_string().unwrap_or(String::new()))
            .and_then(|s| s.parse::<usize>().ok());

        let is_trailer = part.headers.get("Trailer").is_some();

        match (chunked, content_length, pre.is_empty()) {
            (true, None, _) => HttpBody::chunk(pre, Box::new(conn), is_trailer),
            (false, Some(len), _) => HttpBody::text(len, pre, Box::new(conn)),
            (false, None, true) => HttpBody::empty(),
            _ => {
                return err_from_msg!(Request, "Invalid response format");
            }
        }
    };
    Ok(Response::from_raw_parts(part, body))
}

impl<S: Read> Read for Http1Conn<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.raw_mut().read(buf)
    }
}

impl<S: Read> StreamData for Http1Conn<S> {
    fn shutdown(&self) {
        Self::shutdown(self)
    }
}
