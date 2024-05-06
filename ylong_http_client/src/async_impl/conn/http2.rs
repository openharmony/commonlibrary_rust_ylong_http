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

use std::cmp::min;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use ylong_http::error::HttpError;
use ylong_http::h2;
use ylong_http::h2::{ErrorCode, Frame, FrameFlags, H2Error, Payload, PseudoHeaders};
use ylong_http::headers::Headers;
use ylong_http::request::uri::Scheme;
use ylong_http::request::RequestPart;
use ylong_http::response::status::StatusCode;
use ylong_http::response::ResponsePart;

use crate::async_impl::conn::StreamData;
use crate::async_impl::request::Message;
use crate::async_impl::{HttpBody, Response};
use crate::error::{ErrorKind, HttpClientError};
use crate::runtime::{AsyncRead, ReadBuf};
use crate::util::dispatcher::http2::Http2Conn;
use crate::util::h2::{BodyDataRef, RequestWrapper};
use crate::util::normalizer::BodyLengthParser;

const UNUSED_FLAG: u8 = 0x0;

pub(crate) async fn request<S>(
    mut conn: Http2Conn<S>,
    mut message: Message,
) -> Result<Response, HttpClientError>
where
    S: Sync + Send + Unpin + 'static,
{
    message
        .interceptor
        .intercept_request(message.request.ref_mut())?;
    let part = message.request.ref_mut().part().clone();

    // TODO Implement trailer.
    let headers = build_headers_frame(conn.id, part, false)
        .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
    let data = BodyDataRef::new(message.request.clone());
    let stream = RequestWrapper {
        header: headers,
        data,
    };
    conn.send_frame_to_controller(stream)?;
    let frame = conn.receiver.recv().await?;
    frame_2_response(conn, frame, message)
}

fn frame_2_response<S>(
    conn: Http2Conn<S>,
    headers_frame: Frame,
    mut message: Message,
) -> Result<Response, HttpClientError>
where
    S: Sync + Send + Unpin + 'static,
{
    let part = match headers_frame.payload() {
        Payload::Headers(headers) => {
            let (pseudo, fields) = headers.parts();
            let status_code = match pseudo.status() {
                Some(status) => StatusCode::from_bytes(status.as_bytes())
                    .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?,
                None => {
                    return Err(HttpClientError::from_error(
                        ErrorKind::Request,
                        HttpError::from(H2Error::StreamError(conn.id, ErrorCode::ProtocolError)),
                    ));
                }
            };
            ResponsePart {
                version: ylong_http::version::Version::HTTP2,
                status: status_code,
                headers: fields.clone(),
            }
        }
        _ => {
            return Err(HttpClientError::from_error(
                ErrorKind::Request,
                HttpError::from(H2Error::StreamError(conn.id, ErrorCode::ProtocolError)),
            ));
        }
    };

    let text_io = TextIo::new(conn);
    // TODO Whether HTTP2 can have no Content_Length, only whether the END_STREAM
    // flag has a Body
    let length = match BodyLengthParser::new(message.request.ref_mut().method(), &part).parse() {
        Ok(length) => length,
        Err(e) => {
            return Err(e);
        }
    };
    let body = HttpBody::new(message.interceptor, length, Box::new(text_io), &[0u8; 0])?;

    Ok(Response::new(
        ylong_http::response::Response::from_raw_parts(part, body),
    ))
}

pub(crate) fn build_headers_frame(
    id: u32,
    mut part: RequestPart,
    is_end_stream: bool,
) -> Result<Frame, HttpError> {
    remove_connection_specific_headers(&mut part.headers)?;
    let pseudo = build_pseudo_headers(&mut part)?;
    let mut header_part = h2::Parts::new();
    header_part.set_header_lines(part.headers);
    header_part.set_pseudo(pseudo);
    let headers_payload = h2::Headers::new(header_part);

    let mut flag = FrameFlags::new(UNUSED_FLAG);
    flag.set_end_headers(true);
    if is_end_stream {
        flag.set_end_stream(true);
    }
    Ok(Frame::new(
        id as usize,
        flag,
        Payload::Headers(headers_payload),
    ))
}

// Illegal headers validation in http2.
// [`Connection-Specific Headers`] implementation.
//
// [`Connection-Specific Headers`]: https://www.rfc-editor.org/rfc/rfc9113.html#name-connection-specific-header-
fn remove_connection_specific_headers(headers: &mut Headers) -> Result<(), HttpError> {
    const CONNECTION_SPECIFIC_HEADERS: &[&str; 5] = &[
        "connection",
        "keep-alive",
        "proxy-connection",
        "upgrade",
        "transfer-encoding",
    ];
    for specific_header in CONNECTION_SPECIFIC_HEADERS.iter() {
        headers.remove(*specific_header);
    }

    if let Some(te_ref) = headers.get("te") {
        let te = te_ref.to_string()?;
        if te.as_str() != "trailers" {
            headers.remove("te");
        }
    }
    Ok(())
}

fn build_pseudo_headers(request_part: &mut RequestPart) -> Result<PseudoHeaders, HttpError> {
    let mut pseudo = PseudoHeaders::default();
    match request_part.uri.scheme() {
        Some(scheme) => {
            pseudo.set_scheme(Some(String::from(scheme.as_str())));
        }
        None => pseudo.set_scheme(Some(String::from(Scheme::HTTP.as_str()))),
    }
    pseudo.set_method(Some(String::from(request_part.method.as_str())));
    pseudo.set_path(
        request_part
            .uri
            .path_and_query()
            .or_else(|| Some(String::from("/"))),
    );
    let host = request_part
        .headers
        .remove("host")
        .and_then(|auth| auth.to_string().ok());
    pseudo.set_authority(host);
    Ok(pseudo)
}

struct TextIo<S> {
    pub(crate) handle: Http2Conn<S>,
    pub(crate) offset: usize,
    pub(crate) remain: Option<Frame>,
    pub(crate) is_closed: bool,
}

struct HttpReadBuf<'a, 'b> {
    buf: &'a mut ReadBuf<'b>,
}

impl<'a, 'b> HttpReadBuf<'a, 'b> {
    pub(crate) fn append_slice(&mut self, buf: &[u8]) {
        #[cfg(feature = "ylong_base")]
        self.buf.append(buf);

        #[cfg(feature = "tokio_base")]
        self.buf.put_slice(buf);
    }
}

impl<'a, 'b> Deref for HttpReadBuf<'a, 'b> {
    type Target = ReadBuf<'b>;

    fn deref(&self) -> &Self::Target {
        self.buf
    }
}

impl<S> TextIo<S>
where
    S: Sync + Send + Unpin + 'static,
{
    pub(crate) fn new(handle: Http2Conn<S>) -> Self {
        Self {
            handle,
            offset: 0,
            remain: None,
            is_closed: false,
        }
    }
}

impl<S: Sync + Send + Unpin + 'static> StreamData for TextIo<S> {
    fn shutdown(&self) {
        self.handle.io_shutdown.store(true, Ordering::Release);
    }
}

impl<S: Sync + Send + Unpin + 'static> AsyncRead for TextIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let text_io = self.get_mut();
        let mut buf = HttpReadBuf { buf };

        if buf.remaining() == 0 || text_io.is_closed {
            return Poll::Ready(Ok(()));
        }
        while buf.remaining() != 0 {
            if let Some(frame) = &text_io.remain {
                match frame.payload() {
                    Payload::Headers(_) => {
                        break;
                    }
                    Payload::Data(data) => {
                        let data = data.data();
                        let unfilled_len = buf.remaining();
                        let data_len = data.len() - text_io.offset;
                        let fill_len = min(unfilled_len, data_len);
                        if unfilled_len < data_len {
                            buf.append_slice(&data[text_io.offset..text_io.offset + fill_len]);
                            text_io.offset += fill_len;
                            break;
                        } else {
                            buf.append_slice(&data[text_io.offset..text_io.offset + fill_len]);
                            text_io.offset = 0;
                            if frame.flags().is_end_stream() {
                                text_io.is_closed = true;
                                break;
                            }
                        }
                    }
                    _ => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                        )))
                    }
                }
            }

            let poll_result = text_io
                .handle
                .receiver
                .poll_recv(cx)
                .map_err(|_e| std::io::Error::from(std::io::ErrorKind::Other))?;

            match poll_result {
                Poll::Ready(frame) => match frame.payload() {
                    Payload::Headers(_) => {
                        text_io.remain = Some(frame);
                        text_io.offset = 0;
                        break;
                    }
                    Payload::Data(data) => {
                        let data = data.data();
                        let unfilled_len = buf.remaining();
                        let data_len = data.len();
                        let fill_len = min(data_len, unfilled_len);
                        if unfilled_len < data_len {
                            buf.append_slice(&data[..fill_len]);
                            text_io.offset += fill_len;
                            text_io.remain = Some(frame);
                            break;
                        } else {
                            buf.append_slice(&data[..fill_len]);
                            if frame.flags().is_end_stream() {
                                text_io.is_closed = true;
                                break;
                            }
                        }
                    }
                    Payload::RstStream(error) => {
                        if error.is_no_error() {
                            text_io.is_closed = true;
                            break;
                        } else {
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                            )));
                        }
                    }
                    _ => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                        )))
                    }
                },
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "http2")]
#[cfg(test)]
mod ut_http2 {
    use ylong_http::body::TextBody;
    use ylong_http::h2::Payload;
    use ylong_http::request::RequestBuilder;

    use crate::async_impl::conn::http2::build_headers_frame;

    macro_rules! build_request {
        (
            Request: {
                Method: $method: expr,
                Uri: $uri:expr,
                Version: $version: expr,
                $(
                    Header: $req_n: expr, $req_v: expr,
                )*
                Body: $req_body: expr,
            }
        ) => {
            RequestBuilder::new()
                .method($method)
                .url($uri)
                .version($version)
                $(.header($req_n, $req_v))*
                .body(TextBody::from_bytes($req_body.as_bytes()))
                .expect("Request build failed")
        }
    }

    #[test]
    fn ut_http2_build_headers_frame() {
        let request = build_request!(
            Request: {
            Method: "GET",
            Uri: "http://127.0.0.1:0/data",
            Version: "HTTP/2.0",
            Header: "te", "trailers",
            Header: "host", "127.0.0.1:0",
            Body: "Hi",
        }
        );
        let frame = build_headers_frame(1, request.part().clone(), false).unwrap();
        assert_eq!(frame.flags().bits(), 0x4);
        let frame = build_headers_frame(1, request.part().clone(), true).unwrap();
        assert_eq!(frame.stream_id(), 1);
        assert_eq!(frame.flags().bits(), 0x5);
        if let Payload::Headers(headers) = frame.payload() {
            let (pseudo, _headers) = headers.parts();
            assert_eq!(pseudo.status(), None);
            assert_eq!(pseudo.scheme().unwrap(), "http");
            assert_eq!(pseudo.method().unwrap(), "GET");
            assert_eq!(pseudo.authority().unwrap(), "127.0.0.1:0");
            assert_eq!(pseudo.path().unwrap(), "/data")
        } else {
            panic!("Unexpected frame type")
        }
    }
}
