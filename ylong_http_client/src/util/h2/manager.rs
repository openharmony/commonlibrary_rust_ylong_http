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

//! Streams manage coroutine.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use ylong_http::h2::{
    ErrorCode, Frame, FrameFlags, Goaway, H2Error, Payload, Ping, RstStream, Setting, StreamId,
};

use crate::runtime::{BoundedReceiver, UnboundedReceiver, UnboundedSender};
use crate::util::dispatcher::http2::{
    DispatchErrorKind, OutputMessage, ReqMessage, RespMessage, SettingsState, SettingsSync,
    StreamController,
};
use crate::util::h2::streams::{DataReadState, FrameRecvState, StreamEndState};

#[derive(Copy, Clone)]
enum ManagerState {
    Send,
    Receive,
    Exit(DispatchErrorKind),
}

pub(crate) struct ConnManager {
    state: ManagerState,
    next_state: ManagerState,
    // Synchronize SETTINGS frames sent by the client.
    settings: Arc<Mutex<SettingsSync>>,
    // channel transmitter between manager and io input.
    input_tx: UnboundedSender<Frame>,
    // channel receiver between manager and io output.
    resp_rx: BoundedReceiver<OutputMessage>,
    // channel receiver between manager and stream coroutine.
    req_rx: UnboundedReceiver<ReqMessage>,
    controller: StreamController,
}

impl Future for ConnManager {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let manager = self.get_mut();
        loop {
            match manager.state {
                ManagerState::Send => {
                    if manager.poll_blocked_frames(cx).is_pending() {
                        return Poll::Pending;
                    }
                }
                ManagerState::Receive => {
                    // Receives a response frame from io output.
                    match manager.resp_rx.poll_recv(cx) {
                        #[cfg(feature = "tokio_base")]
                        Poll::Ready(Some(message)) => match message {
                            OutputMessage::Output(frame) => {
                                if manager.poll_recv_message(cx, frame)?.is_pending() {
                                    return Poll::Pending;
                                }
                            }
                            // io output occurs error.
                            OutputMessage::OutputExit(e) => {
                                // Ever received a goaway frame
                                if manager.controller.go_away_error_code.is_some() {
                                    continue;
                                }
                                // Note error returned immediately.
                                if manager.manage_resp_error(cx, e)?.is_pending() {
                                    return Poll::Pending;
                                }
                            }
                        },
                        #[cfg(feature = "ylong_base")]
                        Poll::Ready(Ok(message)) => match message {
                            OutputMessage::Output(frame) => {
                                if manager.poll_recv_message(cx, frame)?.is_pending() {
                                    return Poll::Pending;
                                }
                            }
                            // io output occurs error.
                            OutputMessage::OutputExit(e) => {
                                // Ever received a goaway frame
                                if manager.controller.go_away_error_code.is_some() {
                                    continue;
                                }
                                if manager.manage_resp_error(cx, e)?.is_pending() {
                                    return Poll::Pending;
                                }
                            }
                        },
                        #[cfg(feature = "tokio_base")]
                        Poll::Ready(None) => {
                            return manager.poll_channel_closed_exit(cx);
                        }
                        #[cfg(feature = "ylong_base")]
                        Poll::Ready(Err(_e)) => {
                            return manager.poll_channel_closed_exit(cx);
                        }

                        Poll::Pending => {
                            // TODO manage error state.
                            return manager.manage_pending_state(cx);
                        }
                    }
                }
                ManagerState::Exit(e) => return Poll::Ready(Err(e)),
            }
        }
    }
}

impl ConnManager {
    pub(crate) fn new(
        settings: Arc<Mutex<SettingsSync>>,
        input_tx: UnboundedSender<Frame>,
        resp_rx: BoundedReceiver<OutputMessage>,
        req_rx: UnboundedReceiver<ReqMessage>,
        controller: StreamController,
    ) -> Self {
        Self {
            state: ManagerState::Receive,
            next_state: ManagerState::Receive,
            settings,
            input_tx,
            resp_rx,
            req_rx,
            controller,
        }
    }

    fn manage_pending_state(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        // The manager previously accepted a GOAWAY Frame.
        if let Some(error_code) = self.controller.go_away_error_code {
            self.poll_deal_with_go_away(error_code)?;
            return Poll::Pending;
        }
        self.poll_recv_request(cx)?;
        self.poll_input_request(cx)?;
        Poll::Pending
    }

    fn poll_recv_request(&mut self, cx: &mut Context<'_>) -> Result<(), DispatchErrorKind> {
        loop {
            #[cfg(feature = "tokio_base")]
            let message = match self.req_rx.poll_recv(cx) {
                Poll::Ready(Some(message)) => message,
                Poll::Ready(None) => return Err(DispatchErrorKind::ChannelClosed),
                Poll::Pending => break,
            };
            #[cfg(feature = "ylong_base")]
            let message = match self.req_rx.poll_recv(cx) {
                Poll::Ready(Ok(message)) => message,
                Poll::Ready(Err(_e)) => return Err(DispatchErrorKind::ChannelClosed),
                Poll::Pending => break,
            };
            let id = match self.controller.streams.generate_id() {
                Ok(id) => id,
                Err(e) => {
                    let _ = message.sender.try_send(RespMessage::OutputExit(e));
                    break;
                }
            };
            let headers = Frame::new(id, message.request.flag, message.request.payload);
            if self.controller.streams.reach_max_concurrency()
                || !self.controller.streams.is_pending_concurrency_empty()
            {
                self.controller.streams.push_pending_concurrency(id)
            } else {
                self.controller.streams.increase_current_concurrency();
                self.controller.streams.push_back_pending_send(id)
            }
            self.controller.senders.insert(id, message.sender);
            self.controller
                .streams
                .insert(id, headers, message.request.data);
        }
        Ok(())
    }

    fn poll_input_request(&mut self, cx: &mut Context<'_>) -> Result<(), DispatchErrorKind> {
        self.controller.streams.try_consume_pending_concurrency();
        let size = self.controller.streams.pending_stream_num();
        let mut index = 0;
        while index < size {
            match self.controller.streams.next_pending_stream() {
                None => {
                    break;
                }
                Some(id) => {
                    self.input_stream_frame(cx, id)?;
                }
            }
            index += 1;
        }
        Ok(())
    }

    fn input_stream_frame(
        &mut self,
        cx: &mut Context<'_>,
        id: StreamId,
    ) -> Result<(), DispatchErrorKind> {
        match self.controller.streams.headers(id)? {
            None => {}
            Some(header) => {
                let is_end_stream = header.flags().is_end_stream();
                self.poll_send_frame(header)?;
                // Prevent sending empty data frames
                if is_end_stream {
                    return Ok(());
                }
            }
        }

        loop {
            match self.controller.streams.poll_read_body(cx, id) {
                Ok(state) => match state {
                    DataReadState::Closed => break,
                    DataReadState::Pending => break,
                    DataReadState::Ready(data) => self.poll_send_frame(data)?,
                    DataReadState::Finish(frame) => {
                        self.poll_send_frame(frame)?;
                        break;
                    }
                },
                Err(e) => return self.deal_poll_body_error(cx, e),
            }
        }
        Ok(())
    }

    fn deal_poll_body_error(
        &mut self,
        cx: &mut Context<'_>,
        e: H2Error,
    ) -> Result<(), DispatchErrorKind> {
        match e {
            H2Error::StreamError(id, code) => match self.manage_stream_error(cx, id, code) {
                Poll::Ready(res) => res,
                Poll::Pending => Ok(()),
            },
            H2Error::ConnectionError(e) => Err(H2Error::ConnectionError(e).into()),
        }
    }

    fn poll_send_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        match frame.payload() {
            Payload::Headers(_) => {
                if let FrameRecvState::Err(e) = self
                    .controller
                    .streams
                    .send_headers_frame(frame.stream_id(), frame.flags().is_end_stream())
                {
                    // Never return FrameRecvState::Ignore case.
                    return Err(e.into());
                }
            }
            Payload::Data(_) => {
                if let FrameRecvState::Err(e) = self
                    .controller
                    .streams
                    .send_data_frame(frame.stream_id(), frame.flags().is_end_stream())
                {
                    // Never return FrameRecvState::Ignore case.
                    return Err(e.into());
                }
            }
            _ => {}
        }
        // TODO Replace with a bounded channel to avoid excessive local memory overhead
        // when I/O is blocked in the process of uploading large files.
        self.input_tx
            .send(frame)
            .map_err(|_e| DispatchErrorKind::ChannelClosed)
    }

    fn poll_recv_frame(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        match frame.payload() {
            Payload::Settings(_settings) => {
                self.recv_settings_frame(frame)?;
            }
            Payload::Ping(_ping) => {
                self.recv_ping_frame(frame)?;
            }
            Payload::PushPromise(_) => {
                // TODO The current settings_enable_push setting is fixed to false.
                return Poll::Ready(Err(
                    H2Error::ConnectionError(ErrorCode::ProtocolError).into()
                ));
            }
            Payload::Goaway(_go_away) => {
                return self.recv_go_away_frame(cx, frame).map_err(Into::into);
            }
            Payload::RstStream(_reset) => {
                return self.recv_reset_frame(cx, frame).map_err(Into::into);
            }
            Payload::Headers(_headers) => {
                return self.recv_header_frame(cx, frame).map_err(Into::into);
            }
            Payload::Data(_data) => {
                return self.recv_data_frame(cx, frame);
            }
            Payload::WindowUpdate(_windows) => {
                self.recv_window_frame(frame)?;
            }
            // Priority is no longer recommended, so keep it compatible but not processed.
            Payload::Priority(_priority) => {}
        }
        Poll::Ready(Ok(()))
    }

    fn recv_settings_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        let settings = if let Payload::Settings(settings) = frame.payload() {
            settings
        } else {
            // this will not happen forever.
            return Ok(());
        };

        if frame.flags().is_ack() {
            let mut connection = self.settings.lock().unwrap();

            if let SettingsState::Acknowledging(ref acknowledged) = connection.settings {
                for setting in acknowledged.get_settings() {
                    if let Setting::InitialWindowSize(size) = setting {
                        self.controller
                            .streams
                            .apply_recv_initial_window_size(*size);
                    }
                }
            }
            connection.settings = SettingsState::Synced;
            Ok(())
        } else {
            for setting in settings.get_settings() {
                if let Setting::MaxConcurrentStreams(num) = setting {
                    self.controller.streams.apply_max_concurrent_streams(*num);
                }
                if let Setting::InitialWindowSize(size) = setting {
                    self.controller
                        .streams
                        .apply_send_initial_window_size(*size)?;
                }
            }

            // The reason for copying the payload is to pass information to the io input to
            // set the frame encoder, and the input will empty the
            // payload when it is sent
            let ack_settings = Frame::new(
                frame.stream_id(),
                FrameFlags::new(0x1),
                frame.payload().clone(),
            );

            self.input_tx
                .send(ack_settings)
                .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
            Ok(())
        }
    }

    fn recv_ping_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        let ping = if let Payload::Ping(ping) = frame.payload() {
            ping
        } else {
            // this will not happen forever.
            return Ok(());
        };
        if frame.flags().is_ack() {
            // TODO The client does not have the logic to send ping frames. Therefore, the
            // ack ping is not processed.
            Ok(())
        } else {
            self.input_tx
                .send(Ping::ack(ping.clone()))
                .map_err(|_e| DispatchErrorKind::ChannelClosed)
        }
    }

    fn recv_go_away_frame(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), H2Error>> {
        let go_away = if let Payload::Goaway(goaway) = frame.payload() {
            goaway
        } else {
            // this will not happen forever.
            return Poll::Ready(Ok(()));
        };
        // Prevents the current connection from generating a new stream.
        self.controller.goaway();
        self.req_rx.close();
        let last_stream_id = go_away.get_last_stream_id();
        let streams = self.controller.get_unsent_streams(last_stream_id)?;

        let error = H2Error::ConnectionError(ErrorCode::try_from(go_away.get_error_code())?);

        let mut blocked = false;
        for stream_id in streams {
            match self.controller.send_message_to_stream(
                cx,
                stream_id,
                RespMessage::OutputExit(error.into()),
            ) {
                // ignore error when going away.
                Poll::Ready(_) => {}
                Poll::Pending => {
                    blocked = true;
                }
            }
        }
        // Exit after the allowed stream is complete.
        self.controller.go_away_error_code = Some(go_away.get_error_code());
        if blocked {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn recv_reset_frame(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), H2Error>> {
        match self.controller.streams.recv_remote_reset(frame.stream_id()) {
            StreamEndState::OK => self.controller.send_message_to_stream(
                cx,
                frame.stream_id(),
                RespMessage::Output(frame),
            ),
            StreamEndState::Err(e) => Poll::Ready(Err(e)),
            StreamEndState::Ignore => Poll::Ready(Ok(())),
        }
    }

    fn recv_header_frame(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), H2Error>> {
        match self
            .controller
            .streams
            .recv_headers(frame.stream_id(), frame.flags().is_end_stream())
        {
            FrameRecvState::OK => self.controller.send_message_to_stream(
                cx,
                frame.stream_id(),
                RespMessage::Output(frame),
            ),
            FrameRecvState::Err(e) => Poll::Ready(Err(e)),
            FrameRecvState::Ignore => Poll::Ready(Ok(())),
        }
    }

    fn recv_data_frame(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        let data = if let Payload::Data(data) = frame.payload() {
            data
        } else {
            // this will not happen forever.
            return Poll::Ready(Ok(()));
        };
        let id = frame.stream_id();
        let len = data.size() as u32;

        self.update_window(id, len)?;

        match self
            .controller
            .streams
            .recv_data(id, frame.flags().is_end_stream())
        {
            FrameRecvState::OK => self
                .controller
                .send_message_to_stream(cx, frame.stream_id(), RespMessage::Output(frame))
                .map_err(Into::into),
            FrameRecvState::Ignore => Poll::Ready(Ok(())),
            FrameRecvState::Err(e) => Poll::Ready(Err(e.into())),
        }
    }

    fn recv_window_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        let windows = if let Payload::WindowUpdate(windows) = frame.payload() {
            windows
        } else {
            // this will not happen forever.
            return Ok(());
        };
        let id = frame.stream_id();
        let increment = windows.get_increment();
        if id == 0 {
            self.controller
                .streams
                .increase_conn_send_window(increment)?;
            self.controller.streams.reassign_conn_send_window();
        } else {
            self.controller
                .streams
                .reassign_stream_send_window(id, increment)?;
        }
        Ok(())
    }

    fn manage_resp_error(
        &mut self,
        cx: &mut Context<'_>,
        kind: DispatchErrorKind,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        match kind {
            DispatchErrorKind::H2(h2) => match h2 {
                H2Error::StreamError(id, code) => self.manage_stream_error(cx, id, code),
                H2Error::ConnectionError(code) => self.manage_conn_error(cx, code),
            },
            other => {
                let blocked = self.exit_with_error(cx, other);
                if blocked {
                    self.state = ManagerState::Send;
                    self.next_state = ManagerState::Exit(other);
                    Poll::Pending
                } else {
                    Poll::Ready(Err(other))
                }
            }
        }
    }

    fn manage_stream_error(
        &mut self,
        cx: &mut Context<'_>,
        id: StreamId,
        code: ErrorCode,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        let rest_payload = RstStream::new(code.into_code());
        let frame = Frame::new(id, FrameFlags::empty(), Payload::RstStream(rest_payload));
        match self.controller.streams.send_local_reset(id) {
            StreamEndState::OK => {
                self.input_tx
                    .send(frame)
                    .map_err(|_e| DispatchErrorKind::ChannelClosed)?;

                match self.controller.send_message_to_stream(
                    cx,
                    id,
                    RespMessage::OutputExit(DispatchErrorKind::H2(H2Error::StreamError(id, code))),
                ) {
                    Poll::Ready(_) => {
                        // error at the stream level due to early exit of the coroutine in which the
                        // request is located, ignored to avoid manager coroutine exit.
                        Poll::Ready(Ok(()))
                    }
                    Poll::Pending => {
                        self.state = ManagerState::Send;
                        // stream error will not cause manager exit with error(exit state). Takes
                        // effect only if blocked.
                        self.next_state = ManagerState::Receive;
                        Poll::Pending
                    }
                }
            }
            StreamEndState::Ignore => Poll::Ready(Ok(())),
            StreamEndState::Err(e) => {
                // This error will never happen.
                Poll::Ready(Err(e.into()))
            }
        }
    }

    fn manage_conn_error(
        &mut self,
        cx: &mut Context<'_>,
        code: ErrorCode,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        // last_stream_id is set to 0 to ensure that all pushed streams are
        // shutdown.
        let go_away_payload = Goaway::new(
            code.into_code(),
            self.controller.streams.latest_remote_id,
            vec![],
        );
        let frame = Frame::new(
            0,
            FrameFlags::empty(),
            Payload::Goaway(go_away_payload.clone()),
        );
        // Avoid sending the same GO_AWAY frame multiple times.
        if let Some(ref go_away) = self.controller.go_away_sync.going_away {
            if go_away.get_error_code() == go_away_payload.get_error_code()
                && go_away.get_last_stream_id() == go_away_payload.get_last_stream_id()
            {
                return Poll::Ready(Ok(()));
            }
        }
        self.controller.go_away_sync.going_away = Some(go_away_payload);
        self.input_tx
            .send(frame)
            .map_err(|_e| DispatchErrorKind::ChannelClosed)?;

        let blocked =
            self.exit_with_error(cx, DispatchErrorKind::H2(H2Error::ConnectionError(code)));

        if blocked {
            self.state = ManagerState::Send;
            self.next_state = ManagerState::Exit(H2Error::ConnectionError(code).into());
            Poll::Pending
        } else {
            // TODO When current client has an error,
            // it always sends the GO_AWAY frame at the first time and exits directly.
            // Should we consider letting part of the unfinished stream complete?
            Poll::Ready(Err(H2Error::ConnectionError(code).into()))
        }
    }

    fn poll_deal_with_go_away(&mut self, error_code: u32) -> Result<(), DispatchErrorKind> {
        // The client that receives GO_AWAY needs to return a GO_AWAY to the server
        // before closed. The preceding operations before receiving the frame
        // ensure that the connection is in the closing state.
        if self.controller.streams.is_closed() {
            let last_stream_id = self.controller.streams.latest_remote_id;
            let go_away_payload = Goaway::new(error_code, last_stream_id, vec![]);
            let frame = Frame::new(
                0,
                FrameFlags::empty(),
                Payload::Goaway(go_away_payload.clone()),
            );

            self.send_peer_goaway(frame, go_away_payload, error_code)?;
            // close connection
            self.controller.shutdown();
            return Err(H2Error::ConnectionError(ErrorCode::try_from(error_code)?).into());
        }
        Ok(())
    }

    fn send_peer_goaway(
        &mut self,
        frame: Frame,
        payload: Goaway,
        err_code: u32,
    ) -> Result<(), DispatchErrorKind> {
        match self.controller.go_away_sync.going_away {
            None => {
                self.controller.go_away_sync.going_away = Some(payload);
                self.input_tx
                    .send(frame)
                    .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
            }
            Some(ref go_away) => {
                // Whether the same GOAWAY Frame has been sent before.
                if !(go_away.get_error_code() == err_code
                    && go_away.get_last_stream_id() == self.controller.streams.latest_remote_id)
                {
                    self.controller.go_away_sync.going_away = Some(payload);
                    self.input_tx
                        .send(frame)
                        .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
                }
            }
        }
        Ok(())
    }

    fn poll_recv_message(
        &mut self,
        cx: &mut Context<'_>,
        frame: Frame,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        match self.poll_recv_frame(cx, frame) {
            Poll::Ready(Err(kind)) => self.manage_resp_error(cx, kind),
            Poll::Pending => {
                self.state = ManagerState::Send;
                self.next_state = ManagerState::Receive;
                Poll::Pending
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
        }
    }

    fn poll_channel_closed_exit(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        if self.exit_with_error(cx, DispatchErrorKind::ChannelClosed) {
            self.state = ManagerState::Send;
            self.next_state = ManagerState::Exit(DispatchErrorKind::ChannelClosed);
            Poll::Pending
        } else {
            Poll::Ready(Err(DispatchErrorKind::ChannelClosed))
        }
    }

    fn poll_blocked_frames(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self.controller.poll_blocked_message(cx, &self.input_tx) {
            Poll::Ready(_) => {
                self.state = self.next_state;
                // Reset state.
                self.next_state = ManagerState::Receive;
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }

    pub(crate) fn exit_with_error(
        &mut self,
        cx: &mut Context<'_>,
        error: DispatchErrorKind,
    ) -> bool {
        self.controller.shutdown();
        self.req_rx.close();
        self.controller.streams.clear_streams_states();

        let ids = self.controller.streams.get_all_unclosed_streams();
        let mut blocked = false;
        for stream_id in ids {
            match self.controller.send_message_to_stream(
                cx,
                stream_id,
                RespMessage::OutputExit(error),
            ) {
                // ignore error when going away.
                Poll::Ready(_) => {}
                Poll::Pending => {
                    blocked = true;
                }
            }
        }
        blocked
    }

    pub(crate) fn update_window(
        &mut self,
        id: StreamId,
        len: u32,
    ) -> Result<(), DispatchErrorKind> {
        self.controller
            .streams
            .release_conn_recv_window(len, &self.input_tx)?;
        self.controller
            .streams
            .release_stream_recv_window(id, len, &self.input_tx)?;
        Ok(())
    }
}
