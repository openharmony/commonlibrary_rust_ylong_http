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
use ylong_http::h3::{
    Frame, H3Error, H3ErrorCode, Headers, Parts, Payload, PseudoHeaders, HEADERS_FRAME_TYPE,
};
use ylong_http::request::uri::Scheme;
use ylong_http::request::RequestPart;
use ylong_http::response::status::StatusCode;
use ylong_http::response::ResponsePart;
use ylong_runtime::io::ReadBuf;

use crate::async_impl::conn::StreamData;
use crate::async_impl::request::Message;
use crate::async_impl::{HttpBody, Response};
use crate::runtime::AsyncRead;
use crate::util::config::HttpVersion;
use crate::util::data_ref::BodyDataRef;
use crate::util::dispatcher::http3::{DispatchErrorKind, Http3Conn, RequestWrapper, RespMessage};
use crate::util::normalizer::BodyLengthParser;
use crate::ErrorKind::BodyTransfer;
use crate::{ErrorKind, HttpClientError};

pub(crate) async fn request<S>(
    mut conn: Http3Conn<S>,
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
    let headers = build_headers_frame(part)
        .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
    let data = BodyDataRef::new(message.request.clone(), conn.speed_controller.clone());
    let stream = RequestWrapper {
        header: headers,
        data,
    };
    conn.send_frame_to_reader(stream)?;
    let frame = conn.recv_resp().await?;
    frame_2_response(conn, frame, message)
}

pub(crate) fn build_headers_frame(mut part: RequestPart) -> Result<Frame, HttpError> {
    // todo: check rfc to see if any headers should be removed
    let pseudo = build_pseudo_headers(&mut part)?;
    let mut header_part = Parts::new();
    header_part.set_header_lines(part.headers);
    header_part.set_pseudo(pseudo);
    let headers_payload = Headers::new(header_part);

    Ok(Frame::new(
        HEADERS_FRAME_TYPE,
        Payload::Headers(headers_payload),
    ))
}

// todo: error if headers not enough, should meet rfc
fn build_pseudo_headers(request_part: &mut RequestPart) -> Result<PseudoHeaders, HttpError> {
    let mut pseudo = PseudoHeaders::default();
    match request_part.uri.scheme() {
        Some(scheme) => {
            pseudo.set_scheme(Some(String::from(scheme.as_str())));
        }
        None => pseudo.set_scheme(Some(String::from(Scheme::HTTPS.as_str()))),
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

fn frame_2_response<S>(
    conn: Http3Conn<S>,
    headers_frame: Frame,
    mut message: Message,
) -> Result<Response, HttpClientError>
where
    S: Sync + Send + Unpin + 'static,
{
    let part = match headers_frame.payload() {
        Payload::Headers(headers) => {
            let part = headers.get_part();
            let (pseudo, fields) = part.parts();
            let status_code = match pseudo.status() {
                Some(status) => StatusCode::from_bytes(status.as_bytes())
                    .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?,
                None => {
                    return Err(HttpClientError::from_str(
                        ErrorKind::Request,
                        "status code not found",
                    ));
                }
            };
            ResponsePart {
                version: ylong_http::version::Version::HTTP3,
                status: status_code,
                headers: fields.clone(),
            }
        }
        Payload::PushPromise(_) => {
            todo!();
        }
        _ => {
            return Err(HttpClientError::from_str(ErrorKind::Request, "bad frame"));
        }
    };

    let data_io = TextIo::new(conn);
    let length = match BodyLengthParser::new(message.request.ref_mut().method(), &part).parse() {
        Ok(length) => length,
        Err(e) => {
            return Err(e);
        }
    };
    let body = HttpBody::new(message.interceptor, length, Box::new(data_io), &[0u8; 0])?;

    Ok(Response::new(
        ylong_http::response::Response::from_raw_parts(part, body),
    ))
}

struct TextIo<S> {
    pub(crate) handle: Http3Conn<S>,
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
    pub(crate) fn new(handle: Http3Conn<S>) -> Self {
        Self {
            handle,
            offset: 0,
            remain: None,
            is_closed: false,
        }
    }

    fn match_channel_message(
        poll_result: Poll<RespMessage>,
        text_io: &mut TextIo<S>,
        buf: &mut HttpReadBuf,
    ) -> Option<Poll<std::io::Result<()>>> {
        match poll_result {
            Poll::Ready(RespMessage::Output(frame)) => match frame.payload() {
                Payload::Headers(_) => {
                    text_io.remain = Some(frame);
                    text_io.offset = 0;
                    Some(Poll::Ready(Ok(())))
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
                        Some(Poll::Ready(Ok(())))
                    } else {
                        buf.append_slice(&data[..fill_len]);
                        Self::end_read(text_io, data_len)
                    }
                }
                _ => Some(Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    HttpError::from(H3Error::Connection(H3ErrorCode::H3InternalError)),
                )))),
            },
            Poll::Ready(RespMessage::OutputExit(e)) => match e {
                DispatchErrorKind::H3(H3Error::Connection(H3ErrorCode::H3NoError))
                | DispatchErrorKind::StreamFinished => {
                    text_io.is_closed = true;
                    Some(Poll::Ready(Ok(())))
                }
                _ => Some(Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    HttpError::from(H3Error::Connection(H3ErrorCode::H3InternalError)),
                )))),
            },
            Poll::Pending => Some(Poll::Pending),
        }
    }

    fn end_read(text_io: &mut TextIo<S>, data_len: usize) -> Option<Poll<std::io::Result<()>>> {
        text_io.offset = 0;
        text_io.remain = None;
        if data_len == 0 {
            // no data read and is not end stream.
            None
        } else {
            Some(Poll::Ready(Ok(())))
        }
    }

    fn read_remaining_data(
        text_io: &mut TextIo<S>,
        buf: &mut HttpReadBuf,
    ) -> Option<Poll<std::io::Result<()>>> {
        if let Some(frame) = &text_io.remain {
            return match frame.payload() {
                Payload::Headers(_) => Some(Poll::Ready(Ok(()))),
                Payload::Data(data) => {
                    let data = data.data();
                    let unfilled_len = buf.remaining();
                    let data_len = data.len() - text_io.offset;
                    let fill_len = min(unfilled_len, data_len);
                    // The peripheral function already ensures that the remaing of buf will not be
                    // 0.
                    if unfilled_len < data_len {
                        buf.append_slice(&data[text_io.offset..text_io.offset + fill_len]);
                        text_io.offset += fill_len;
                        Some(Poll::Ready(Ok(())))
                    } else {
                        buf.append_slice(&data[text_io.offset..text_io.offset + fill_len]);
                        Self::end_read(text_io, data_len)
                    }
                }
                _ => Some(Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    HttpError::from(H3Error::Connection(H3ErrorCode::H3InternalError)),
                )))),
            };
        }
        None
    }
}

impl<S: Sync + Send + Unpin + 'static> StreamData for TextIo<S> {
    fn shutdown(&self) {
        self.handle.io_shutdown.store(true, Ordering::Relaxed);
    }

    fn is_stream_closable(&self) -> bool {
        self.is_closed
    }

    fn http_version(&self) -> HttpVersion {
        HttpVersion::Http3
    }
}

impl<S: Sync + Send + Unpin + 'static> AsyncRead for TextIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut buf = HttpReadBuf { buf };
        let text_io = self.get_mut();
        if buf.remaining() == 0 || text_io.is_closed {
            return Poll::Ready(Ok(()));
        }
        if text_io
            .handle
            .speed_controller
            .poll_recv_pending_timeout(cx)
        {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                HttpClientError::from_str(BodyTransfer, "Below low speed limit"),
            )));
        }
        text_io.handle.speed_controller.init_min_recv_if_not_start();
        if text_io
            .handle
            .speed_controller
            .poll_max_recv_delay_time(cx)
            .is_pending()
        {
            return Poll::Pending;
        }
        text_io.handle.speed_controller.init_max_recv_if_not_start();
        while buf.remaining() != 0 {
            if let Some(result) = Self::read_remaining_data(text_io, &mut buf) {
                return match result {
                    Poll::Ready(Ok(_)) => {
                        let filled: usize = buf.filled().len();
                        text_io
                            .handle
                            .speed_controller
                            .min_recv_speed_limit(filled)
                            .map_err(|_e| std::io::Error::from(std::io::ErrorKind::Other))?;
                        text_io
                            .handle
                            .speed_controller
                            .delay_max_recv_speed_limit(filled);
                        text_io.handle.speed_controller.reset_recv_pending_timeout();
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                };
            }

            let poll_result = text_io
                .handle
                .resp_receiver
                .poll_recv(cx)
                .map_err(|_e| std::io::Error::from(std::io::ErrorKind::ConnectionAborted))?;

            if let Some(result) = Self::match_channel_message(poll_result, text_io, &mut buf) {
                return match result {
                    Poll::Ready(Ok(_)) => {
                        let filled: usize = buf.filled().len();
                        text_io
                            .handle
                            .speed_controller
                            .min_recv_speed_limit(filled)
                            .map_err(|_e| std::io::Error::from(std::io::ErrorKind::Other))?;
                        text_io
                            .handle
                            .speed_controller
                            .delay_max_recv_speed_limit(filled);
                        text_io.handle.speed_controller.reset_recv_pending_timeout();
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                };
            }
        }
        Poll::Ready(Ok(()))
    }
}
