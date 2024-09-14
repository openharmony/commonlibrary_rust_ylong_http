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

//! Stream Manager module.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use quiche::Shutdown;
use ylong_http::h3::{
    Frame, FrameDecoder, FrameEncoder, FrameKind, Frames, H3Error, H3ErrorCode, Headers, Payload,
    Settings, StreamMessage, CONTROL_STREAM_TYPE, QPACK_DECODER_STREAM_TYPE,
    QPACK_ENCODER_STREAM_TYPE, SETTINGS_FRAME_TYPE,
};

use crate::async_impl::QuicConn;
use crate::runtime::{UnboundedReceiver, UnboundedSender};
use crate::util::config::H3Config;
use crate::util::dispatcher::http3::{DispatchErrorKind, ReqMessage, RespMessage};
use crate::util::h3::streams::{DataReadState, QUICStreamType, Streams};

pub(crate) const UPD_RECV_BUF_SIZE: usize = 65535;
const DECODE_BUF_SIZE: usize = 1024;

pub(crate) struct StreamManager {
    pub(crate) streams: Streams,
    pub(crate) quic_conn: Arc<Mutex<QuicConn>>,
    pub(crate) io_manager_tx: UnboundedSender<Result<(), DispatchErrorKind>>,
    pub(crate) stream_manager_rx: UnboundedReceiver<Result<(), DispatchErrorKind>>,
    pub(crate) req_rx: UnboundedReceiver<ReqMessage>,
    pub(crate) stream_recv_buf: [u8; UPD_RECV_BUF_SIZE],
    pub(crate) encoder: FrameEncoder,
    pub(crate) decoder: FrameDecoder,
    pub(crate) encoder_buf: [u8; DECODE_BUF_SIZE],
    pub(crate) inst_buf: [u8; DECODE_BUF_SIZE],
    pub(crate) peer_settings: Option<Settings>,
    pub(crate) io_shutdown: Arc<AtomicBool>,
    pub(crate) io_goaway: Arc<AtomicBool>,
}

impl StreamManager {
    pub(crate) fn new(
        quic_conn: Arc<Mutex<QuicConn>>,
        io_manager_tx: UnboundedSender<Result<(), DispatchErrorKind>>,
        stream_manager_rx: UnboundedReceiver<Result<(), DispatchErrorKind>>,
        req_rx: UnboundedReceiver<ReqMessage>,
        decoder: FrameDecoder,
        io_shutdown: Arc<AtomicBool>,
        io_goaway: Arc<AtomicBool>,
    ) -> Self {
        Self {
            streams: Streams::new(),
            quic_conn,
            io_manager_tx,
            stream_manager_rx,
            req_rx,
            stream_recv_buf: [0u8; UPD_RECV_BUF_SIZE],
            encoder_buf: [0u8; DECODE_BUF_SIZE],
            inst_buf: [0u8; DECODE_BUF_SIZE],
            encoder: FrameEncoder::default(),
            decoder,
            peer_settings: None,
            io_shutdown,
            io_goaway,
        }
    }

    fn poll_recv_signal(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Result<(), DispatchErrorKind>, DispatchErrorKind>> {
        #[cfg(feature = "tokio_base")]
        match self.stream_manager_rx.poll_recv(cx) {
            Poll::Ready(None) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Some(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
        #[cfg(feature = "ylong_base")]
        match self.stream_manager_rx.poll_recv(cx) {
            Poll::Ready(Err(_e)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Ok(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_recv_request(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<ReqMessage, DispatchErrorKind>> {
        #[cfg(feature = "tokio_base")]
        match self.req_rx.poll_recv(cx) {
            Poll::Ready(None) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Some(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
        #[cfg(feature = "ylong_base")]
        match self.req_rx.poll_recv(cx) {
            Poll::Ready(Err(_e)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Ok(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn send_inst_to_peer(
        &mut self,
        headers: &Headers,
        quic_conn: &mut QuicConn,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(vec) = headers.get_instruction() {
            let qpack_decode_stream_id =
                self.streams
                    .qpack_decode_stream_id()
                    .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3InternalError,
                    )))?;
            quic_conn.stream_send(qpack_decode_stream_id, vec, false)?;
        }
        Ok(())
    }

    fn transmit_error(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        error: DispatchErrorKind,
    ) -> Result<(), DispatchErrorKind> {
        self.streams.send_error(cx, id, error)
    }

    fn poll_input_request(&mut self, cx: &mut Context<'_>) -> Result<(), DispatchErrorKind> {
        self.streams.try_consume_pending_concurrency();
        let len = self.streams.pending_stream_len();
        // Some streams may be blocked due to the server not reading the message. Avoid
        // reading these streams twice in one loop
        for _ in 0..len {
            if let Some(id) = self.streams.next_stream() {
                self.input_stream_frame(cx, id)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn input_stream_frame(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(header) = self.streams.get_header(id)? {
            self.poll_send_header(id, header)?;
        }

        // encoding means last frame is still encoding, can not create new frame before
        // consumed.
        if self.streams.encoding(id)? {
            if let Err(e) = self.poll_send_frame(id, None) {
                return match e {
                    DispatchErrorKind::Quic(quiche::Error::StreamStopped(_)) => Ok(()),
                    e => Err(e),
                };
            }
            if self.streams.encoding(id)? {
                self.streams.push_back_pending_send(id);
                return Ok(());
            }
        }

        loop {
            match self.poll_read_body(cx, id)? {
                DataReadState::Closed | DataReadState::Pending => {
                    break;
                }
                DataReadState::Ready(data) => {
                    if let Err(e) = self.poll_send_frame(id, Some(*data)) {
                        return match e {
                            DispatchErrorKind::Quic(quiche::Error::StreamStopped(_)) => Ok(()),
                            e => Err(e),
                        };
                    }
                    if self.streams.encoding(id)? {
                        self.streams.push_back_pending_send(id);
                        break;
                    }
                }
                DataReadState::Finish => {
                    let mut quic_conn = self.quic_conn.lock().unwrap();
                    quic_conn.stream_send(id, b"", true)?;
                    let _ = self.io_manager_tx.send(Ok(()));
                    break;
                }
            }
        }
        Ok(())
    }

    fn poll_send_header(&mut self, id: u64, frame: Frame) -> Result<(), DispatchErrorKind> {
        self.streams.set_encoding(id, true)?;
        self.encoder.set_frame(id, frame)?;
        let quic_conn = self.quic_conn.clone();
        let qpack_encode_stream_id =
            self.streams
                .qpack_encode_stream_id()
                .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3InternalError,
                )))?;
        let mut quic_conn = quic_conn.lock().unwrap();

        // invalid means stream has not been created, create it first
        if let Err(quiche::Error::InvalidStreamState(_)) =
            quic_conn.stream_writable(id, DECODE_BUF_SIZE)
        {
            quic_conn.stream_send(id, b"", false)?;
        }
        while quic_conn.stream_writable(id, DECODE_BUF_SIZE)?
            && quic_conn.stream_writable(qpack_encode_stream_id, DECODE_BUF_SIZE)?
        {
            let (data_size, inst_size) =
                self.encoder
                    .encode(id, &mut self.encoder_buf, &mut self.inst_buf)?;
            if inst_size != 0 {
                quic_conn.stream_send(
                    qpack_encode_stream_id,
                    &self.inst_buf[..inst_size],
                    false,
                )?;
            }
            if data_size != 0 {
                quic_conn.stream_send(id, &self.encoder_buf[..data_size], false)?;
            }
            if inst_size == 0 && data_size == 0 {
                self.streams.set_encoding(id, false)?;
                break;
            }
        }

        let _ = self.io_manager_tx.send(Ok(()));
        Ok(())
    }

    fn poll_send_frame(&mut self, id: u64, frame: Option<Frame>) -> Result<(), DispatchErrorKind> {
        if let Some(frame) = frame {
            self.streams.set_encoding(id, true)?;
            self.encoder.set_frame(id, frame)?;
        }
        let mut quic_conn = self.quic_conn.lock().unwrap();

        loop {
            if !quic_conn.stream_writable(id, DECODE_BUF_SIZE)? {
                break;
            }
            let (data_size, _) =
                self.encoder
                    .encode(id, &mut self.encoder_buf, &mut self.inst_buf)?;
            if data_size != 0 {
                quic_conn.stream_send(id, &self.encoder_buf[..data_size], false)?;
                let _ = self.io_manager_tx.send(Ok(()));
            } else {
                self.streams.set_encoding(id, false)?;
                break;
            }
        }
        Ok(())
    }

    pub(crate) fn poll_read_body(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
    ) -> Result<DataReadState, DispatchErrorKind> {
        const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024;
        let len = std::cmp::min(
            self.quic_conn
                .lock()
                .unwrap()
                .stream_capacity(id)
                .map_err(|_| {
                    DispatchErrorKind::H3(H3Error::Stream(id, H3ErrorCode::H3InternalError))
                })?,
            DEFAULT_MAX_FRAME_SIZE,
        );
        let mut buf = [0u8; DEFAULT_MAX_FRAME_SIZE];
        self.streams.poll_sized_data(cx, id, &mut buf[..len])
    }

    pub(crate) fn init(&mut self, config: H3Config) -> Result<(), DispatchErrorKind> {
        self.decoder
            .local_allowed_max_field_section_size(config.max_field_section_size() as usize);
        self.send_settings(config)?;
        self.open_uni_stream(QPACK_ENCODER_STREAM_TYPE)?;
        self.open_uni_stream(QPACK_DECODER_STREAM_TYPE)?;
        Ok(())
    }

    pub(crate) fn open_uni_stream(&mut self, stream_type: u8) -> Result<u64, DispatchErrorKind> {
        let buf = [stream_type];
        let id = match stream_type {
            CONTROL_STREAM_TYPE => {
                self.streams
                    .control_stream_id()
                    .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3InternalError,
                    )))?
            }
            QPACK_ENCODER_STREAM_TYPE => {
                self.streams
                    .qpack_encode_stream_id()
                    .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3InternalError,
                    )))?
            }
            QPACK_DECODER_STREAM_TYPE => {
                self.streams
                    .qpack_decode_stream_id()
                    .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3InternalError,
                    )))?
            }
            _ => {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3InternalError,
                )))
            }
        };
        let mut quic_conn = self.quic_conn.lock().unwrap();

        quic_conn.stream_send(id, &buf, false)?;
        let _ = quic_conn.stream_priority(id, 0, false);
        Ok(id)
    }

    pub(crate) fn send_settings(&mut self, config: H3Config) -> Result<(), DispatchErrorKind> {
        let control_stream_id = self.open_uni_stream(CONTROL_STREAM_TYPE)?;

        let mut settings = Settings::default();
        settings.set_max_field_section_size(config.max_field_section_size());
        settings.set_qpack_max_table_capacity(config.qpack_max_table_capacity());
        settings.set_qpack_block_stream(config.qpack_blocked_streams());

        let mut quic_conn = self.quic_conn.lock().unwrap();
        let settings = Frame::new(SETTINGS_FRAME_TYPE, Payload::Settings(settings));
        self.encoder.set_frame(control_stream_id, settings)?;
        loop {
            let (size, _) = self.encoder.encode(
                control_stream_id,
                &mut self.encoder_buf,
                &mut self.inst_buf,
            )?;
            if size == 0 {
                return Ok(());
            }
            quic_conn.stream_send(control_stream_id, &self.encoder_buf[..size], false)?;
        }
    }

    pub(crate) fn poll_stream_recv(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Result<(), DispatchErrorKind> {
        let mut need_send = false;
        let lock = self.quic_conn.clone();
        let mut quic_conn = lock.lock().unwrap();

        if let Some(stream_id) = self.streams.peer_control_stream_id() {
            need_send |= self.try_recv_uni_stream(cx, &mut quic_conn, stream_id)?;
        };
        if let Some(stream_id) = self.streams.peer_qpack_encode_stream_id() {
            need_send |= self.try_recv_uni_stream(cx, &mut quic_conn, stream_id)?;
        };
        if let Some(stream_id) = self.streams.peer_qpack_decode_stream_id() {
            need_send |= self.try_recv_uni_stream(cx, &mut quic_conn, stream_id)?;
        };
        for id in quic_conn.readable() {
            if !self.streams.frame_acceptable(id) {
                continue;
            }
            need_send |= self.read_stream(cx, &mut quic_conn, id)?;
        }

        if quic_conn.is_closed() {
            self.shutdown(cx, &DispatchErrorKind::Disconnect);
        }

        if need_send {
            let _ = self.io_manager_tx.send(Ok(()));
        }
        Ok(())
    }

    fn try_recv_uni_stream(
        &mut self,
        cx: &mut Context<'_>,
        quic_conn: &mut QuicConn,
        stream_id: u64,
    ) -> Result<bool, DispatchErrorKind> {
        if quic_conn.stream_finished(stream_id) {
            return Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3ClosedCriticalStream,
            )));
        }

        match self.read_stream(cx, quic_conn, stream_id) {
            Ok(need_send) => {
                if quic_conn.stream_finished(stream_id) {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3ClosedCriticalStream,
                    )));
                }
                Ok(need_send)
            }
            Err(e) => Err(e),
        }
    }

    fn read_stream(
        &mut self,
        cx: &mut Context<'_>,
        quic_conn: &mut QuicConn,
        id: u64,
    ) -> Result<bool, DispatchErrorKind> {
        if QUICStreamType::from(id) == QUICStreamType::ServerInitialBidirectional {
            return Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3StreamCreationError,
            )));
        }
        let mut need_send = false;
        loop {
            let (size, fin) = match quic_conn.stream_recv(id, &mut self.stream_recv_buf) {
                Ok((size, fin)) => {
                    need_send = true;
                    (size, fin)
                }
                Err(quiche::Error::Done) => {
                    return Ok(need_send);
                }
                Err(quiche::Error::StreamStopped(err)) | Err(quiche::Error::StreamReset(err)) => {
                    if err != H3ErrorCode::H3NoError as u64 {
                        return Err(DispatchErrorKind::H3(H3Error::Stream(id, err.into())));
                    } else {
                        return Ok(false);
                    }
                }
                Err(e) => {
                    return Err(DispatchErrorKind::Quic(e));
                }
            };
            self.process_recv_data(cx, id, size, quic_conn)?;
            if fin {
                self.finish_stream(cx, id)?;
                return Ok(true);
            }
        }
    }

    fn process_recv_data(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        size: usize,
        quic_conn: &mut QuicConn,
    ) -> Result<(), DispatchErrorKind> {
        let mut stream_id = id;
        let mut size = size;
        loop {
            match self
                .decoder
                .decode(stream_id, &self.stream_recv_buf[..size])
            {
                Ok(StreamMessage::Request(frames)) => {
                    self.recv_request_stream(cx, stream_id, frames, quic_conn)?;
                }
                Ok(StreamMessage::Push(_id, _frames)) => {
                    // MAX_PUSH_ID not send, Push Stream means error
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3IdError,
                    )));
                }
                Ok(StreamMessage::QpackDecoder(order)) => {
                    self.recv_qpack_decode_stream(stream_id, order)?;
                }
                Ok(StreamMessage::Control(frames)) => {
                    self.recv_control_stream(cx, stream_id, frames)?;
                }
                Ok(StreamMessage::WaitingMore) | Ok(StreamMessage::Unknown) => {}
                Ok(StreamMessage::QpackEncoder(vec)) => {
                    self.recv_qpack_encode_stream(stream_id, vec)?;
                }
                Err(e) => {
                    self.transmit_error(cx, stream_id, DispatchErrorKind::H3(e))?;
                }
            }
            if let Some(id) = self.streams.get_resume_stream_id() {
                stream_id = id;
            } else {
                return Ok(());
            };
            size = 0;
        }
    }

    fn recv_qpack_encode_stream(
        &mut self,
        stream_id: u64,
        vec: Vec<u64>,
    ) -> Result<(), DispatchErrorKind> {
        self.streams.set_peer_qpack_encode_stream_id(stream_id)?;
        for resume_id in vec {
            self.streams.resume_stream_recv(resume_id);
        }
        Ok(())
    }

    fn recv_request_stream(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        frames: Frames,
        quic_conn: &mut QuicConn,
    ) -> Result<(), DispatchErrorKind> {
        for kind in frames.into_iter() {
            let frame = match kind {
                FrameKind::Complete(frame) => frame,
                FrameKind::Blocked => {
                    self.streams.pend_stream_recv(id);
                    return Ok(());
                }
                FrameKind::Partial => return Ok(()),
            };
            match frame.payload() {
                Payload::Headers(headers) => {
                    self.send_inst_to_peer(headers, quic_conn)?;
                    self.streams.send_frame(cx, id, *frame)?;
                }
                Payload::Data(_) => {
                    self.streams.send_frame(cx, id, *frame)?;
                }
                Payload::PushPromise(_) => {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3IdError,
                    )))
                }
                _ => {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3FrameUnexpected,
                    )))
                }
            }
        }
        Ok(())
    }

    fn recv_control_stream(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        frames: Frames,
    ) -> Result<(), DispatchErrorKind> {
        let mut is_first_frame = if let Some(stream_id) = self.streams.peer_control_stream_id() {
            if stream_id != id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3StreamCreationError,
                )));
            }
            false
        } else {
            self.streams.set_peer_control_stream_id(id)?;
            true
        };
        for frame in frames.iter() {
            let FrameKind::Complete(frame) = frame else {
                continue;
            };
            match frame.payload() {
                Payload::Settings(settings) => {
                    self.recv_setting_frame(settings)?;
                    is_first_frame = false;
                }
                Payload::Goaway(goaway) => {
                    self.recv_goaway_frame(cx, *goaway.get_id())?;
                }
                Payload::CancelPush(_cancel) => {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3IdError,
                    )));
                }
                _ => {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3FrameUnexpected,
                    )));
                }
            }
            if is_first_frame {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3MissingSettings,
                )));
            }
        }
        Ok(())
    }

    fn recv_qpack_decode_stream(
        &mut self,
        stream_id: u64,
        order: Vec<u8>,
    ) -> Result<(), DispatchErrorKind> {
        self.streams.set_peer_qpack_decode_stream_id(stream_id)?;
        self.encoder.decode_remote_inst(&order)?;
        Ok(())
    }

    fn recv_setting_frame(&mut self, settings: &Settings) -> Result<(), DispatchErrorKind> {
        if self.peer_settings.is_some() {
            return Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3FrameUnexpected,
            )));
        }
        self.peer_settings = Some(settings.clone());
        if let Some(value) = settings.qpack_max_table_capacity() {
            self.encoder.set_max_table_capacity(value as usize)?;
        }
        if let Some(value) = settings.qpack_block_stream() {
            self.encoder.set_max_blocked_stream_size(value as usize);
        }
        Ok(())
    }

    fn recv_goaway_frame(
        &mut self,
        cx: &mut Context<'_>,
        goaway_id: u64,
    ) -> Result<(), DispatchErrorKind> {
        self.io_goaway.store(true, Ordering::Relaxed);
        self.req_rx.close();
        self.streams.goaway(cx, goaway_id)?;
        Ok(())
    }

    fn handle_error(&mut self, cx: &mut Context<'_>, err: &DispatchErrorKind) -> bool {
        match err {
            DispatchErrorKind::H3(H3Error::Stream(id, e)) => {
                self.handle_stream_error(cx, *id, e);
                false
            }
            DispatchErrorKind::Quic(quiche::Error::InvalidStreamState(id)) => {
                self.handle_stream_error(cx, *id, &H3ErrorCode::H3NoError);
                false
            }
            err => {
                self.handle_connection_error(cx, err);
                true
            }
        }
    }

    fn handle_stream_error(&mut self, cx: &mut Context<'_>, id: u64, err: &H3ErrorCode) {
        let _ = self
            .quic_conn
            .lock()
            .unwrap()
            .stream_shutdown(id, Shutdown::Read, *err as u64);
        self.streams.shutdown_stream(cx, id, err);
    }

    fn handle_connection_error(&mut self, cx: &mut Context<'_>, err: &DispatchErrorKind) {
        self.shutdown(cx, err);
        let err = match err {
            DispatchErrorKind::H3(H3Error::Connection(err)) => *err,
            _ => H3ErrorCode::H3InternalError,
        };
        let _ = self.quic_conn.lock().unwrap().close(true, err as u64, b"");
        let _ = self.io_manager_tx.send(Ok(()));
        self.req_rx.close();
    }

    fn shutdown(&mut self, cx: &mut Context<'_>, err: &DispatchErrorKind) {
        self.io_shutdown.store(true, Ordering::Relaxed);
        self.streams.shutdown(cx, err);
    }

    fn finish_stream(&mut self, cx: &mut Context<'_>, id: u64) -> Result<(), DispatchErrorKind> {
        self.streams.finish_stream(cx, id)?;
        self.encoder.finish_stream(id)?;
        self.decoder.finish_stream(id)?;
        if self.streams.goaway_id().is_some() && self.streams.current_concurrency() == 0 {
            self.io_shutdown.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub(crate) fn poll_blocked_message(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        self.streams.poll_blocked_message(cx)
    }
}

impl Future for StreamManager {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            // 1 recv stream_manager_rx, meaning data to send/recv
            match this.poll_recv_signal(cx) {
                // consume all the signals
                Poll::Ready(Ok(Ok(()))) => continue,
                Poll::Ready(Ok(Err(e))) | Poll::Ready(Err(e)) => {
                    if this.handle_error(cx, &e) {
                        return Poll::Ready(Err(e));
                    }
                }
                Poll::Pending => {}
            }

            // 2 check id's channel sendable / control/qpack/push, decode, send or cache
            // frame if stream_recv, send io_manager_tx to io manager
            if let Err(e) = this.poll_stream_recv(cx) {
                if this.handle_error(cx, &e) {
                    return Poll::Ready(Err(e));
                }
            }

            if let Poll::Ready(Err(e)) = this.poll_blocked_message(cx) {
                if this.handle_error(cx, &e) {
                    return Poll::Ready(Err(e));
                }
            }

            // 3 recv req_rx, check concurrency
            loop {
                let req = match this.poll_recv_request(cx) {
                    Poll::Ready(Ok(req)) => req,
                    Poll::Ready(Err(e)) => {
                        if this.handle_error(cx, &e) {
                            return Poll::Ready(Err(e));
                        }
                        break;
                    }
                    Poll::Pending => break,
                };
                if let Err(e) = this.streams.new_unidirectional_stream(
                    req.request.header,
                    req.request.data,
                    req.frame_tx.clone(),
                ) {
                    let _ = req.frame_tx.try_send(RespMessage::OutputExit(e));
                }
            }

            // 4 in concurrency stream, set frame to encoder, set encoding flag, get encode
            // result send flag to io manager
            if let Err(e) = this.poll_input_request(cx) {
                if this.handle_error(cx, &e) {
                    return Poll::Ready(Err(e));
                }
            }
            return Poll::Pending;
        }
    }
}
