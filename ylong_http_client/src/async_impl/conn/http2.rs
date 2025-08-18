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
use std::mem::take;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use std::time::Instant;

use ylong_http::error::HttpError;
use ylong_http::h2;
use ylong_http::h2::{ErrorCode, Frame, FrameFlags, H2Error, Payload, PseudoHeaders, StreamId};
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
use crate::util::config::HttpVersion;
use crate::util::data_ref::BodyDataRef;
use crate::util::dispatcher::http2::Http2Conn;
use crate::util::h2::RequestWrapper;
use crate::util::normalizer::BodyLengthParser;
use crate::ErrorKind::BodyTransfer;

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
    let is_body_empty = message.request.ref_mut().body().is_empty();
    let no_length = message.request.ref_mut().headers().get("content-length").is_none();
    let (flag, payload) = build_headers_payload(part, is_body_empty && no_length)
        .map_err(|e| HttpClientError::from_error(ErrorKind::Request, e))?;
    let data = BodyDataRef::new(message.request.clone(), conn.speed_controller.clone());
    let stream = RequestWrapper {
        flag,
        payload,
        data,
    };
    message
        .request
        .ref_mut()
        .time_group_mut()
        .set_transfer_start(Instant::now());
    conn.send_frame_to_controller(stream)?;
    let frame = conn.receiver.recv().await?;
    message
        .request
        .ref_mut()
        .time_group_mut()
        .set_transfer_end(Instant::now());
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
                    return Err(build_client_error(
                        headers_frame.stream_id(),
                        ErrorCode::ProtocolError,
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
            return Err(build_client_error(
                headers_frame.stream_id(),
                ErrorCode::try_from(reset.error_code()).unwrap_or(ErrorCode::ProtocolError),
            ));
        }
        _ => {
            return Err(build_client_error(
                headers_frame.stream_id(),
                ErrorCode::ProtocolError,
            ));
        }
    };

    let text_io = TextIo::new(conn);
    let length = match BodyLengthParser::new(message.request.ref_mut().method(), &part).parse() {
        Ok(length) => length,
        Err(e) => {
            return Err(e);
        }
    };
    let time_group = take(message.request.ref_mut().time_group_mut());
    let body = HttpBody::new(message.interceptor, length, Box::new(text_io), &[0u8; 0])?;

    let mut response = Response::new(ylong_http::response::Response::from_raw_parts(part, body));
    response.set_time_group(time_group);
    Ok(response)
}

pub(crate) fn build_headers_payload(
    mut part: RequestPart,
    is_end_stream: bool,
) -> Result<(FrameFlags, Payload), HttpError> {
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
    Ok((flag, Payload::Headers(headers_payload)))
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

fn build_client_error(id: StreamId, code: ErrorCode) -> HttpClientError {
    HttpClientError::from_error(
        ErrorKind::Request,
        HttpError::from(H2Error::StreamError(id, code)),
    )
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

    fn match_channel_message(
        poll_result: Poll<Frame>,
        text_io: &mut TextIo<S>,
        buf: &mut HttpReadBuf,
    ) -> Option<Poll<std::io::Result<()>>> {
        match poll_result {
            Poll::Ready(frame) => match frame.payload() {
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
                        Self::end_read(text_io, frame.flags().is_end_stream(), data_len)
                    }
                }
                Payload::RstStream(reset) => {
                    if reset.is_no_error() {
                        text_io.is_closed = true;
                        Some(Poll::Ready(Ok(())))
                    } else {
                        Some(Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                        ))))
                    }
                }
                _ => Some(Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                )))),
            },
            Poll::Pending => Some(Poll::Pending),
        }
    }

    fn end_read(
        text_io: &mut TextIo<S>,
        end_stream: bool,
        data_len: usize,
    ) -> Option<Poll<std::io::Result<()>>> {
        text_io.offset = 0;
        text_io.remain = None;
        if end_stream {
            text_io.is_closed = true;
            Some(Poll::Ready(Ok(())))
        } else if data_len == 0 {
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
                        Self::end_read(text_io, frame.flags().is_end_stream(), data_len)
                    }
                }
                _ => Some(Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    HttpError::from(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                )))),
            };
        }
        None
    }
}

impl<S: Sync + Send + Unpin + 'static> StreamData for TextIo<S> {
    fn shutdown(&self) {
        self.handle.io_shutdown.store(true, Ordering::Release);
    }

    fn is_stream_closable(&self) -> bool {
        self.is_closed
    }

    fn http_version(&self) -> HttpVersion {
        HttpVersion::Http2
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
        // Min speed contains the max speed limit sleep time.
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
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
                .receiver
                .poll_recv(cx)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            if let Some(result) = Self::match_channel_message(poll_result, text_io, &mut buf) {
                return match result {
                    Poll::Ready(Ok(_)) => {
                        let filled: usize = buf.filled().len();
                        text_io
                            .handle
                            .speed_controller
                            .min_recv_speed_limit(filled)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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

#[cfg(feature = "http2")]
#[cfg(test)]
mod ut_http2 {
    use ylong_http::body::TextBody;
    use ylong_http::h2::Payload;
    use ylong_http::request::RequestBuilder;

    use crate::async_impl::conn::http2::build_headers_payload;

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
    fn ut_http2_build_headers_payload() {
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
        let (flag, _) = build_headers_payload(request.part().clone(), false).unwrap();
        assert_eq!(flag.bits(), 0x4);
        let (flag, payload) = build_headers_payload(request.part().clone(), true).unwrap();
        assert_eq!(flag.bits(), 0x5);
        if let Payload::Headers(headers) = payload {
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

    /// UT for ensure that the response body(data frame) can read ends normally.
    ///
    /// # Brief
    /// 1. Creates three data frames, one greater than buf, one less than buf,
    ///    and the last one equal to and finished with buf.
    /// 2. The response body data is read from TextIo using a buf of 10 bytes.
    /// 3. The body is all read, and the size is the same as the default.
    /// 5. Checks that result.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_http2_body_poll_read() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        use std::pin::Pin;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use ylong_http::h2::{Data, Frame, FrameFlags};
        use ylong_runtime::futures::poll_fn;
        use ylong_runtime::io::{AsyncRead, ReadBuf};

        use crate::async_impl::conn::http2::TextIo;
        use crate::util::dispatcher::http2::Http2Conn;
        use crate::{ConnDetail, ConnProtocol};

        let (resp_tx, resp_rx) = ylong_runtime::sync::mpsc::bounded_channel(20);
        let (req_tx, _req_rx) = crate::runtime::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let detail = ConnDetail {
            protocol: ConnProtocol::Tcp,
            local: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443),
            addr: "localhost".to_string(),
        };
        let mut conn: Http2Conn<()> = Http2Conn::new(20, shutdown, req_tx, detail);
        conn.receiver.set_receiver(resp_rx);
        let mut text_io = TextIo::new(conn);
        let data_1 = Frame::new(
            1,
            FrameFlags::new(0),
            Payload::Data(Data::new(vec![b'a'; 128])),
        );
        let data_2 = Frame::new(
            1,
            FrameFlags::new(0),
            Payload::Data(Data::new(vec![b'a'; 2])),
        );
        let data_3 = Frame::new(
            1,
            FrameFlags::new(1),
            Payload::Data(Data::new(vec![b'a'; 10])),
        );

        ylong_runtime::block_on(async {
            let _ = resp_tx
                .send(crate::util::dispatcher::http2::RespMessage::Output(data_1))
                .await;
            let _ = resp_tx
                .send(crate::util::dispatcher::http2::RespMessage::Output(data_2))
                .await;
            let _ = resp_tx
                .send(crate::util::dispatcher::http2::RespMessage::Output(data_3))
                .await;
        });

        ylong_runtime::block_on(async {
            let mut buf = [0_u8; 10];
            let mut output_vec = vec![];

            let mut size = buf.len();
            // `output_vec < 1024` in order to be able to exit normally in case of an
            // exception.
            while size != 0 && output_vec.len() < 1024 {
                let mut buffer = ReadBuf::new(buf.as_mut_slice());
                poll_fn(|cx| Pin::new(&mut text_io).poll_read(cx, &mut buffer))
                    .await
                    .unwrap();
                size = buffer.filled_len();
                output_vec.extend_from_slice(&buf[..size]);
            }
            assert_eq!(output_vec.len(), 140);
        })
    }
}
