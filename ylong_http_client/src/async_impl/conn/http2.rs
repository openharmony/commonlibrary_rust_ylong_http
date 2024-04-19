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
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use ylong_http::body::async_impl::Body;
use ylong_http::error::HttpError;
use ylong_http::h2;
use ylong_http::h2::{ErrorCode, Frame, FrameFlags, H2Error, Payload, PseudoHeaders};
use ylong_http::headers::Headers;
use ylong_http::request::uri::Scheme;
use ylong_http::request::{Request, RequestPart};
use ylong_http::response::status::StatusCode;
use ylong_http::response::{Response, ResponsePart};

use crate::async_impl::client::Retryable;
use crate::async_impl::conn::HttpBody;
use crate::async_impl::request::Message;
use crate::async_impl::StreamData;
use crate::error::{ErrorKind, HttpClientError};
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf};
use crate::util::dispatcher::http2::Http2Conn;

const UNUSED_FLAG: u8 = 0x0;

pub(crate) async fn request<S, T>(
    mut conn: Http2Conn<S>,
    message: Message<T>,
    retryable: &mut Retryable,
) -> Result<Response<HttpBody>, HttpClientError>
where
    T: Body,
    S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
{
    let part = message.request.part().clone();
    let body = message.request.body_mut();

    // TODO Due to the reason of the Body structure, the use of the trailer is not
    // implemented here for the time being, and it needs to be completed after the
    // Body trait is provided to obtain the trailer interface
    match build_data_frame(conn.id as usize, body).await? {
        None => {
            let headers = build_headers_frame(conn.id, part, true)
                .map_err(|e| HttpClientError::from_error(ErrorKind::Request, Some(e)))?;
            conn.send_frame_to_controller(headers).map_err(|e| {
                retryable.set_retry(true);
                HttpClientError::from_error(ErrorKind::Request, Some(e))
            })?;
        }
        Some(data) => {
            let headers = build_headers_frame(conn.id, part, false)
                .map_err(|e| HttpClientError::from_error(ErrorKind::Request, Some(e)))?;
            conn.send_frame_to_controller(headers).map_err(|e| {
                retryable.set_retry(true);
                HttpClientError::from_error(ErrorKind::Request, Some(e))
            })?;
            conn.send_frame_to_controller(data).map_err(|e| {
                retryable.set_retry(true);
                HttpClientError::from_error(ErrorKind::Request, Some(e))
            })?;
        }
    }
    let frame = Pin::new(&mut conn.stream_info)
        .await
        .map_err(|e| HttpClientError::from_error(ErrorKind::Request, Some(e)))?;
    frame_2_response(conn, frame, retryable)
}

fn frame_2_response<S, T>(
    conn: Http2Conn<S>,
    headers_frame: Frame,
    retryable: &mut Retryable,
    message: Message<T>,
) -> Result<Response<HttpBody>, HttpClientError>
where
    S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
{
    let part = match headers_frame.payload() {
        Payload::Headers(headers) => {
            let (pseudo, fields) = headers.parts();
            let status_code = match pseudo.status() {
                Some(status) => StatusCode::from_bytes(status.as_bytes())
                    .map_err(|e| HttpClientError::from_error(ErrorKind::Request, Some(e)))?,
                None => {
                    return Err(HttpClientError::from_error(
                        ErrorKind::Request,
                        Some(HttpError::from(H2Error::StreamError(
                            conn.id,
                            ErrorCode::ProtocolError,
                        ))),
                    ));
                }
            };
            ResponsePart {
                version: ylong_http::version::Version::HTTP2,
                status: status_code,
                headers: fields.clone(),
            }
        }
        Payload::RstStream(reset) => {
            return Err(HttpClientError::from_error(
                ErrorKind::Request,
                Some(HttpError::from(reset.error(conn.id).map_err(|e| {
                    HttpClientError::from_error(ErrorKind::Request, Some(e))
                })?)),
            ));
        }
        Payload::Goaway(_) => {
            // return Err(HttpClientError::from(ErrorKind::Resend));
            retryable.set_retry(true);
            return Err(HttpClientError::from_str(ErrorKind::Request, "GoAway"));
        }
        _ => {
            return Err(HttpClientError::from_error(
                ErrorKind::Request,
                Some(HttpError::from(H2Error::StreamError(
                    conn.id,
                    ErrorCode::ProtocolError,
                ))),
            ));
        }
    };

    let body = {
        if headers_frame.flags().is_end_stream() {
            HttpBody::empty()
        } else {
            // TODO Can Content-Length in h2 be null?
            let content_length = part
                .headers
                .get("Content-Length")
                .map(|v| v.to_string().unwrap_or(String::new()))
                .and_then(|s| s.parse::<usize>().ok());
            match content_length {
                None => HttpBody::empty(),
                Some(0) => HttpBody::empty(),
                Some(size) => {
                    let text_io = TextIo::new(conn);
                    HttpBody::text(size, &[0u8; 0], Box::new(text_io), message.interceptor)
                }
            }
        }
    };
    Ok(Response::from_raw_parts(part, body))
}

pub(crate) async fn build_data_frame<T: Body>(
    id: usize,
    body: &mut T,
) -> Result<Option<Frame>, HttpClientError> {
    let mut data_vec = vec![];
    let mut buf = [0u8; 1024];
    loop {
        let size = body
            .data(&mut buf)
            .await
            .map_err(|e| HttpClientError::from_error(ErrorKind::Request, Some(e)))?;
        if size == 0 {
            break;
        }
        data_vec.extend_from_slice(&buf[..size]);
    }
    if data_vec.is_empty() {
        Ok(None)
    } else {
        // TODO When the Body trait supports trailer, END_STREAM_FLAG needs to be
        // modified
        let mut flag = FrameFlags::new(UNUSED_FLAG);
        flag.set_end_stream(true);
        Ok(Some(Frame::new(
            id,
            flag,
            Payload::Data(h2::Data::new(data_vec)),
        )))
    }
}

pub(crate) fn build_headers_frame(
    id: u32,
    part: RequestPart,
    is_end_stream: bool,
) -> Result<Frame, HttpError> {
    check_connection_specific_headers(id, &part.headers)?;
    let pseudo = build_pseudo_headers(&part);
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
fn check_connection_specific_headers(id: u32, headers: &Headers) -> Result<(), HttpError> {
    const CONNECTION_SPECIFIC_HEADERS: &[&str; 5] = &[
        "connection",
        "keep-alive",
        "proxy-connection",
        "upgrade",
        "transfer-encoding",
    ];
    for specific_header in CONNECTION_SPECIFIC_HEADERS.iter() {
        if headers.get(*specific_header).is_some() {
            return Err(H2Error::StreamError(id, ErrorCode::ProtocolError).into());
        }
    }
    if let Some(te_value) = headers.get("te") {
        if te_value.to_string()? != "trailers" {
            return Err(H2Error::StreamError(id, ErrorCode::ProtocolError).into());
        }
    }
    Ok(())
}

fn build_pseudo_headers(request_part: &RequestPart) -> PseudoHeaders {
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
    // TODO Validity verification is required, for example: `Authority` must be
    // consistent with the `Host` header
    pseudo.set_authority(request_part.uri.authority().map(|auth| auth.to_string()));
    pseudo
}

struct TextIo<S> {
    pub(crate) handle: Http2Conn<S>,
    pub(crate) offset: usize,
    pub(crate) remain: Option<Frame>,
    pub(crate) is_closed: bool,
}

impl<S> TextIo<S> {
    pub(crate) fn new(handle: Http2Conn<S>) -> Self {
        Self {
            handle,
            offset: 0,
            remain: None,
            is_closed: false,
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static> StreamData for TextIo<S> {
    fn shutdown(&self) {
        todo!()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static> AsyncRead for TextIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let text_io = self.get_mut();

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
                            buf.put_slice(&data[text_io.offset..text_io.offset + fill_len]);
                            text_io.offset += fill_len;
                            break;
                        } else {
                            buf.put_slice(&data[text_io.offset..text_io.offset + fill_len]);
                            text_io.offset += fill_len;
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

            let poll_result = Pin::new(&mut text_io.handle.stream_info)
                .poll(cx)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            // TODO Added the frame type.
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
                            buf.put_slice(&data[..fill_len]);
                            text_io.offset += fill_len;
                            text_io.remain = Some(frame);
                            break;
                        } else {
                            buf.put_slice(&data[..fill_len]);
                            if frame.flags().is_end_stream() {
                                text_io.is_closed = true;
                                break;
                            }
                        }
                    }
                    Payload::RstStream(_) => {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                        )))
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
    use ylong_http::h2::{ErrorCode, H2Error, Payload};
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
        let frame = build_headers_frame(1, request.part().clone(), false)
            .expect("headers frame build failed");
        assert_eq!(frame.flags().bits(), 0x4);
        let frame = build_headers_frame(1, request.part().clone(), true)
            .expect("headers frame build failed");
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
        let request = build_request!(
            Request: {
            Method: "GET",
            Uri: "http://127.0.0.1:0/data",
            Version: "HTTP/2.0",
            Header: "upgrade", "h2",
            Header: "host", "127.0.0.1:0",
            Body: "Hi",
        }
        );
        let frame = build_headers_frame(1, request.part().clone(), true);
        assert_eq!(
            frame.err(),
            Some(H2Error::StreamError(1, ErrorCode::ProtocolError).into())
        );
    }
}
