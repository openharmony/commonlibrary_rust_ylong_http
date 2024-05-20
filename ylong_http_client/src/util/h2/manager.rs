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
    ErrorCode, Frame, FrameFlags, Goaway, H2Error, Payload, Ping, RstStream, Setting,
};

use crate::runtime::{UnboundedReceiver, UnboundedSender};
use crate::util::dispatcher::http2::{
    DispatchErrorKind, OutputMessage, ReqMessage, RespMessage, SettingsState, SettingsSync,
    StreamController,
};
use crate::util::h2::streams::{DataReadState, FrameRecvState, StreamEndState};

pub(crate) struct ConnManager {
    // Synchronize SETTINGS frames sent by the client.
    settings: Arc<Mutex<SettingsSync>>,
    // channel transmitter between manager and io input.
    input_tx: UnboundedSender<Frame>,
    // channel receiver between manager and io output.
    resp_rx: UnboundedReceiver<OutputMessage>,
    // channel receiver between manager and stream coroutine.
    req_rx: UnboundedReceiver<ReqMessage>,
    controller: StreamController,
}

impl Future for ConnManager {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let manager = self.get_mut();
        loop {
            // Receives a response frame from io output.
            match manager.resp_rx.poll_recv(cx) {
                #[cfg(feature = "tokio_base")]
                Poll::Ready(Some(message)) => match message {
                    OutputMessage::Output(frame) => {
                        manager.poll_recv_message(frame)?;
                    }
                    // io output occurs error.
                    OutputMessage::OutputExit(e) => {
                        manager.manage_resp_error(e)?;
                    }
                },
                #[cfg(feature = "ylong_base")]
                Poll::Ready(Ok(message)) => match message {
                    OutputMessage::Output(frame) => {
                        manager.poll_recv_message(frame)?;
                    }
                    // io output occurs error.
                    OutputMessage::OutputExit(e) => {
                        manager.manage_resp_error(e)?;
                    }
                },
                #[cfg(feature = "tokio_base")]
                Poll::Ready(None) => {
                    manager.exit_with_error(DispatchErrorKind::ChannelClosed);
                    return Poll::Ready(Ok(()));
                }
                #[cfg(feature = "ylong_base")]
                Poll::Ready(Err(_e)) => {
                    manager.exit_with_error(DispatchErrorKind::ChannelClosed);
                    return Poll::Ready(Ok(()));
                }

                Poll::Pending => {
                    return manager.manage_pending_state(cx);
                }
            }
        }
    }
}

impl ConnManager {
    pub(crate) fn new(
        settings: Arc<Mutex<SettingsSync>>,
        input_tx: UnboundedSender<Frame>,
        resp_rx: UnboundedReceiver<OutputMessage>,
        req_rx: UnboundedReceiver<ReqMessage>,
        controller: StreamController,
    ) -> Self {
        Self {
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
        if let Some(code) = self.controller.go_away {
            self.poll_deal_with_go_away(code)?;
        }
        self.controller.streams.window_update_conn(&self.input_tx)?;
        self.controller
            .streams
            .window_update_streams(&self.input_tx)?;
        self.poll_recv_request(cx)?;
        self.poll_input_request(cx)?;
        Poll::Pending
    }

    fn poll_recv_request(&mut self, cx: &mut Context<'_>) -> Result<(), DispatchErrorKind> {
        loop {
            #[cfg(feature = "tokio_base")]
            match self.req_rx.poll_recv(cx) {
                Poll::Ready(Some(message)) => {
                    if self.controller.streams.reach_max_concurrency() {
                        self.controller.streams.push_pending_concurrency(message.id);
                    } else {
                        self.controller.streams.increase_current_concurrency();
                        self.controller.streams.push_back_pending_send(message.id)
                    }
                    self.controller.senders.insert(message.id, message.sender);
                    self.controller.streams.insert(message.id, message.request);
                }
                Poll::Ready(None) => {
                    return Err(DispatchErrorKind::ChannelClosed);
                }
                Poll::Pending => {
                    break;
                }
            }
            #[cfg(feature = "ylong_base")]
            match self.req_rx.poll_recv(cx) {
                Poll::Ready(Ok(message)) => {
                    if self.controller.streams.reach_max_concurrency() {
                        self.controller.streams.push_pending_concurrency(message.id);
                    } else {
                        self.controller.streams.increase_current_concurrency();
                        self.controller.streams.push_back_pending_send(message.id)
                    }
                    self.controller.senders.insert(message.id, message.sender);
                    self.controller.streams.insert(message.id, message.request);
                }
                Poll::Ready(Err(_e)) => {
                    return Err(DispatchErrorKind::ChannelClosed);
                }
                Poll::Pending => {
                    break;
                }
            }
        }
        Ok(())
    }

    fn poll_input_request(&mut self, cx: &mut Context<'_>) -> Result<(), DispatchErrorKind> {
        loop {
            self.controller.streams.try_consume_pending_concurrency();
            match self.controller.streams.next_stream() {
                None => {
                    break;
                }
                Some(id) => {
                    self.input_stream_frame(cx, id)?;
                }
            }
        }
        Ok(())
    }

    fn input_stream_frame(
        &mut self,
        cx: &mut Context<'_>,
        id: u32,
    ) -> Result<(), DispatchErrorKind> {
        match self.controller.streams.headers(id)? {
            None => {}
            Some(header) => {
                self.poll_send_frame(header)?;
            }
        }

        loop {
            match self.controller.streams.poll_read_body(cx, id)? {
                DataReadState::Closed => {
                    break;
                }
                DataReadState::Pending => {
                    break;
                }
                DataReadState::Ready(data) => {
                    self.poll_send_frame(data)?;
                }
                DataReadState::Finish(frame) => {
                    self.poll_send_frame(frame)?;
                    break;
                }
            }
        }
        Ok(())
    }

    fn poll_send_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        match frame.payload() {
            Payload::Headers(_) => {
                if let FrameRecvState::Err(e) = self
                    .controller
                    .streams
                    .send_headers_frame(frame.stream_id() as u32, frame.flags().is_end_stream())
                {
                    // Never return FrameRecvState::Ignore case.
                    return Err(e.into());
                }
            }
            Payload::Data(_) => {
                if let FrameRecvState::Err(e) = self
                    .controller
                    .streams
                    .send_data_frame(frame.stream_id() as u32, frame.flags().is_end_stream())
                {
                    // Never return FrameRecvState::Ignore case.
                    return Err(e.into());
                }
            }
            _ => {}
        }

        self.input_tx
            .send(frame)
            .map_err(|_e| DispatchErrorKind::ChannelClosed)
    }

    fn poll_recv_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        match frame.payload() {
            Payload::Settings(_settings) => {
                self.recv_settings_frame(frame)?;
            }
            Payload::Ping(_ping) => {
                self.recv_ping_frame(frame)?;
            }
            Payload::PushPromise(_) => {
                // TODO The current settings_enable_push setting is fixed to false.
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError).into());
            }
            Payload::Goaway(_go_away) => {
                self.recv_go_away_frame(frame)?;
            }
            Payload::RstStream(_reset) => {
                self.recv_reset_frame(frame)?;
            }
            Payload::Headers(_headers) => {
                self.recv_header_frame(frame)?;
            }
            Payload::Data(_data) => {
                self.recv_data_frame(frame)?;
            }
            Payload::WindowUpdate(_windows) => {
                self.recv_window_frame(frame)?;
            }
            // Priority is no longer recommended, so keep it compatible but not processed.
            Payload::Priority(_priority) => {}
        }
        Ok(())
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
            let new_settings = Frame::new(
                frame.stream_id(),
                FrameFlags::new(0x1),
                frame.payload().clone(),
            );
            self.input_tx
                .send(new_settings)
                .map_err(|_e| DispatchErrorKind::ChannelClosed)
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

    fn recv_go_away_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        let go_away = if let Payload::Goaway(goaway) = frame.payload() {
            goaway
        } else {
            // this will not happen forever.
            return Ok(());
        };
        // Prevents the current connection from generating a new stream.
        self.controller.shutdown();
        self.req_rx.close();
        let last_stream_id = go_away.get_last_stream_id();
        let streams = self
            .controller
            .go_away_unsent_stream(last_stream_id as u32)?;

        let error = H2Error::ConnectionError(ErrorCode::try_from(go_away.get_error_code())?);
        for stream_id in streams {
            self.controller
                .send_message_to_stream(stream_id, RespMessage::OutputExit(error.clone().into()));
        }
        // Exit after the allowed stream is complete.
        self.controller.go_away = Some(go_away.get_error_code());
        Ok(())
    }

    fn recv_reset_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        match self
            .controller
            .streams
            .recv_remote_reset(frame.stream_id() as u32)
        {
            StreamEndState::OK => {
                self.controller
                    .send_message_to_stream(frame.stream_id() as u32, RespMessage::Output(frame));
            }
            StreamEndState::Err(e) => {
                return Err(e.into());
            }
            _ => {}
        }
        Ok(())
    }

    fn recv_header_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        match self
            .controller
            .streams
            .recv_headers(frame.stream_id() as u32, frame.flags().is_end_stream())
        {
            FrameRecvState::OK => {
                self.controller
                    .send_message_to_stream(frame.stream_id() as u32, RespMessage::Output(frame));
            }
            FrameRecvState::Err(e) => {
                return Err(e.into());
            }
            _ => {}
        }
        Ok(())
    }

    fn recv_data_frame(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        let data = if let Payload::Data(data) = frame.payload() {
            data
        } else {
            // this will not happen forever.
            return Ok(());
        };
        let id = frame.stream_id() as u32;
        let len = data.size() as u32;
        match self
            .controller
            .streams
            .recv_data(id, frame.flags().is_end_stream())
        {
            FrameRecvState::OK => {
                self.controller
                    .send_message_to_stream(frame.stream_id() as u32, RespMessage::Output(frame));
            }
            FrameRecvState::Ignore => {}
            FrameRecvState::Err(e) => return Err(e.into()),
        }
        self.controller.streams.release_conn_recv_window(len)?;
        self.controller
            .streams
            .release_stream_recv_window(id, len)?;
        Ok(())
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
                .reassign_stream_send_window(id as u32, increment)?;
        }
        Ok(())
    }

    fn manage_resp_error(&mut self, kind: DispatchErrorKind) -> Result<(), DispatchErrorKind> {
        match kind {
            DispatchErrorKind::H2(h2) => {
                match h2 {
                    H2Error::StreamError(id, code) => {
                        self.manage_stream_error(id, code)?;
                    }
                    H2Error::ConnectionError(code) => {
                        self.manage_conn_error(code)?;
                    }
                }
                Ok(())
            }
            other => {
                self.exit_with_error(other.clone());
                Err(other)
            }
        }
    }

    fn manage_stream_error(&mut self, id: u32, code: ErrorCode) -> Result<(), DispatchErrorKind> {
        let rest_payload = RstStream::new(code.into_code());
        let frame = Frame::new(
            id as usize,
            FrameFlags::empty(),
            Payload::RstStream(rest_payload),
        );
        match self.controller.streams.send_local_reset(id) {
            StreamEndState::OK => {
                self.input_tx
                    .send(frame)
                    .map_err(|_e| DispatchErrorKind::ChannelClosed)?;

                self.controller.send_message_to_stream(
                    id,
                    RespMessage::OutputExit(DispatchErrorKind::ChannelClosed),
                );
            }
            StreamEndState::Ignore => {}
            StreamEndState::Err(e) => {
                // This error will never happen.
                return Err(e.into());
            }
        }
        Ok(())
    }

    fn manage_conn_error(&mut self, code: ErrorCode) -> Result<(), DispatchErrorKind> {
        self.exit_with_error(DispatchErrorKind::H2(H2Error::ConnectionError(
            code.clone(),
        )));

        // last_stream_id is set to 0 to ensure that all streams are
        // shutdown.
        let go_away_payload = Goaway::new(
            code.clone().into_code(),
            self.controller.streams.latest_remote_id as usize,
            vec![],
        );
        let frame = Frame::new(
            0,
            FrameFlags::empty(),
            Payload::Goaway(go_away_payload.clone()),
        );
        if let Some(ref go_away) = self.controller.go_away_sync.going_away {
            if go_away.get_error_code() == go_away_payload.get_error_code()
                && go_away.get_last_stream_id() == go_away_payload.get_last_stream_id()
            {
                return Ok(());
            }
        }
        // Avoid sending the same GO_AWAY frame multiple times.
        self.controller.go_away_sync.going_away = Some(go_away_payload);
        self.input_tx
            .send(frame)
            .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
        // TODO When the current client has an error,
        // it always sends the GO_AWAY frame at the first time and exits directly.
        // Should we consider letting part of the unfinished stream complete?
        Err(H2Error::ConnectionError(code).into())
    }

    fn poll_deal_with_go_away(&mut self, error_code: u32) -> Result<(), DispatchErrorKind> {
        // The client that receives GO_AWAY needs to return a GO_AWAY to the server
        // before closed. The preceding operations before receiving the frame
        // ensure that the connection is in the closing state.
        if self.controller.streams.is_closed() {
            let last_stream_id = self.controller.streams.latest_remote_id as usize;
            let go_away_payload = Goaway::new(error_code, last_stream_id, vec![]);
            let frame = Frame::new(
                0,
                FrameFlags::empty(),
                Payload::Goaway(go_away_payload.clone()),
            );

            self.send_peer_goaway(frame, go_away_payload, error_code)?;
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
                    && go_away.get_last_stream_id()
                        == self.controller.streams.latest_remote_id as usize)
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

    fn poll_recv_message(&mut self, frame: Frame) -> Result<(), DispatchErrorKind> {
        if let Err(kind) = self.poll_recv_frame(frame) {
            self.manage_resp_error(kind)?;
        }
        Ok(())
    }

    pub(crate) fn exit_with_error(&mut self, error: DispatchErrorKind) {
        self.controller.shutdown();
        self.req_rx.close();
        self.controller
            .streams
            .go_away_all_streams(&mut self.controller.senders, error);
    }
}
