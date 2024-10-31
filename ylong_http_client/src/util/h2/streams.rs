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

//! Streams operations utils.

use std::cmp::{min, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};

use ylong_http::h2::{Data, ErrorCode, Frame, FrameFlags, H2Error, Payload, StreamId};

use crate::runtime::UnboundedSender;
use crate::util::data_ref::BodyDataRef;
use crate::util::dispatcher::http2::DispatchErrorKind;
use crate::util::h2::buffer::{FlowControl, RecvWindow, SendWindow};

pub(crate) const INITIAL_MAX_SEND_STREAM_ID: StreamId = u32::MAX >> 1;
pub(crate) const INITIAL_MAX_RECV_STREAM_ID: StreamId = u32::MAX >> 1;

const DEFAULT_MAX_STREAM_ID: StreamId = u32::MAX >> 1;
const INITIAL_LATEST_REMOTE_ID: StreamId = 0;
const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 100;

pub(crate) enum FrameRecvState {
    OK,
    Ignore,
    Err(H2Error),
}

pub(crate) enum DataReadState {
    Closed,
    // Wait for poll_read or wait for window.
    Pending,
    Ready(Frame),
    Finish(Frame),
}

pub(crate) enum StreamEndState {
    OK,
    Ignore,
    Err(H2Error),
}

//                              +--------+
//                      send PP |        | recv PP
//                     ,--------|  idle  |--------.
//                    /         |        |         \
//                   v          +--------+          v
//            +----------+          |           +----------+
//            |          |          | send H /  |          |
//     ,------| reserved |          | recv H    | reserved |------.
//     |      | (local)  |          |           | (remote) |      |
//     |      +----------+          v           +----------+      |
//     |          |             +--------+             |          |
//     |          |     recv ES |        | send ES     |          |
//     |   send H |     ,-------|  open  |-------.     | recv H   |
//     |          |    /        |        |        \    |          |
//     |          v   v         +--------+         v   v          |
//     |      +----------+          |           +----------+      |
//     |      |   half   |          |           |   half   |      |
//     |      |  closed  |          | send R /  |  closed  |      |
//     |      | (remote) |          | recv R    | (local)  |      |
//     |      +----------+          |           +----------+      |
//     |           |                |                 |           |
//     |           | send ES /      |       recv ES / |           |
//     |           | send R /       v        send R / |           |
//     |           | recv R     +--------+   recv R   |           |
//     | send R /  `----------->|        |<-----------'  send R / |
//     | recv R                 | closed |               recv R   |
//     `----------------------->|        |<----------------------'
//                              +--------+
#[derive(Copy, Clone, Debug)]
pub(crate) enum H2StreamState {
    Idle,
    // When response does not depend on request,
    // the server can send response directly without waiting for the request to finish receiving.
    // Therefore, the sending and receiving states of the client have their own states
    Open {
        send: ActiveState,
        recv: ActiveState,
    },
    #[allow(dead_code)]
    ReservedRemote,
    // After the request is sent, the state is waiting for the response to be received.
    LocalHalfClosed(ActiveState),
    // When the response is received but the request is not fully sent,
    // this indicates the status of the request being sent
    RemoteHalfClosed(ActiveState),
    Closed(CloseReason),
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum CloseReason {
    LocalRst,
    RemoteRst,
    RemoteGoAway,
    LocalGoAway,
    EndStream,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum ActiveState {
    WaitHeaders,
    WaitData,
}

pub(crate) struct Stream {
    pub(crate) recv_window: RecvWindow,
    pub(crate) send_window: SendWindow,
    pub(crate) state: H2StreamState,
    pub(crate) header: Option<Frame>,
    pub(crate) data: BodyDataRef,
}

pub(crate) struct RequestWrapper {
    pub(crate) flag: FrameFlags,
    pub(crate) payload: Payload,
    pub(crate) data: BodyDataRef,
}

pub(crate) struct Streams {
    // Records the received goaway last_stream_id.
    pub(crate) max_send_id: StreamId,
    // Records the send goaway last_stream_id.
    pub(crate) max_recv_id: StreamId,
    // Currently the client doesn't support push promise, so this value is always 0.
    pub(crate) latest_remote_id: StreamId,
    pub(crate) stream_recv_window_size: u32,
    pub(crate) stream_send_window_size: u32,
    max_concurrent_streams: u32,
    current_concurrent_streams: u32,
    flow_control: FlowControl,
    pending_concurrency: VecDeque<StreamId>,
    pending_stream_window: HashSet<u32>,
    pending_conn_window: VecDeque<u32>,
    pending_send: VecDeque<StreamId>,
    window_updating_streams: VecDeque<StreamId>,
    pub(crate) stream_map: HashMap<StreamId, Stream>,
    pub(crate) next_stream_id: StreamId,
}

macro_rules! change_stream_state {
    (Idle: $eos: expr, $state: expr) => {
        $state = if $eos {
            H2StreamState::RemoteHalfClosed(ActiveState::WaitHeaders)
        } else {
            H2StreamState::Open {
                send: ActiveState::WaitHeaders,
                recv: ActiveState::WaitData,
            }
        };
    };
    (Open: $eos: expr, $state: expr, $send: expr) => {
        $state = if $eos {
            H2StreamState::RemoteHalfClosed($send.clone())
        } else {
            H2StreamState::Open {
                send: $send.clone(),
                recv: ActiveState::WaitData,
            }
        };
    };
    (HalfClosed: $eos: expr, $state: expr) => {
        $state = if $eos {
            H2StreamState::Closed(CloseReason::EndStream)
        } else {
            H2StreamState::LocalHalfClosed(ActiveState::WaitData)
        };
    };
}

impl Streams {
    pub(crate) fn new(
        recv_window_size: u32,
        send_window_size: u32,
        flow_control: FlowControl,
    ) -> Self {
        Self {
            max_send_id: INITIAL_MAX_SEND_STREAM_ID,
            max_recv_id: INITIAL_MAX_RECV_STREAM_ID,
            latest_remote_id: INITIAL_LATEST_REMOTE_ID,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            current_concurrent_streams: 0,
            stream_recv_window_size: recv_window_size,
            stream_send_window_size: send_window_size,
            flow_control,
            pending_concurrency: VecDeque::new(),
            pending_stream_window: HashSet::new(),
            pending_conn_window: VecDeque::new(),
            pending_send: VecDeque::new(),
            window_updating_streams: VecDeque::new(),
            stream_map: HashMap::new(),
            next_stream_id: 1,
        }
    }

    pub(crate) fn decrease_current_concurrency(&mut self) {
        self.current_concurrent_streams -= 1;
    }

    pub(crate) fn increase_current_concurrency(&mut self) {
        self.current_concurrent_streams += 1;
    }

    pub(crate) fn reach_max_concurrency(&mut self) -> bool {
        self.current_concurrent_streams >= self.max_concurrent_streams
    }

    pub(crate) fn apply_max_concurrent_streams(&mut self, num: u32) {
        self.max_concurrent_streams = num;
    }

    pub(crate) fn apply_send_initial_window_size(&mut self, size: u32) -> Result<(), H2Error> {
        let current = self.stream_send_window_size;
        self.stream_send_window_size = size;

        match current.cmp(&size) {
            Ordering::Less => {
                let excess = size - current;
                for (_id, stream) in self.stream_map.iter_mut() {
                    stream.send_window.increase_size(excess)?;
                }
                for id in self.pending_stream_window.iter() {
                    self.pending_send.push_back(*id);
                }
                self.pending_stream_window.clear();
            }
            Ordering::Greater => {
                let excess = current - size;
                for (_id, stream) in self.stream_map.iter_mut() {
                    stream.send_window.reduce_size(excess);
                }
            }
            Ordering::Equal => {}
        }
        Ok(())
    }

    pub(crate) fn apply_recv_initial_window_size(&mut self, size: u32) {
        let current = self.stream_recv_window_size;
        self.stream_recv_window_size = size;
        match current.cmp(&size) {
            Ordering::Less => {
                for (_id, stream) in self.stream_map.iter_mut() {
                    let extra = size - current;
                    stream.recv_window.increase_notification(extra);
                    stream.recv_window.increase_actual(extra);
                }
            }
            Ordering::Greater => {
                for (_id, stream) in self.stream_map.iter_mut() {
                    stream.recv_window.reduce_notification(current - size);
                }
            }
            Ordering::Equal => {}
        }
    }

    pub(crate) fn release_stream_recv_window(
        &mut self,
        id: StreamId,
        size: u32,
    ) -> Result<(), H2Error> {
        if let Some(stream) = self.stream_map.get_mut(&id) {
            if stream.recv_window.notification_available() < size {
                return Err(H2Error::StreamError(id, ErrorCode::FlowControlError));
            }
            stream.recv_window.recv_data(size);
            if stream.recv_window.unreleased_size().is_some() {
                self.window_updating_streams.push_back(id);
            }
        }
        Ok(())
    }

    pub(crate) fn release_conn_recv_window(&mut self, size: u32) -> Result<(), H2Error> {
        if self.flow_control.recv_notification_size_available() < size {
            return Err(H2Error::ConnectionError(ErrorCode::FlowControlError));
        }
        self.flow_control.recv_data(size);
        Ok(())
    }

    pub(crate) fn is_closed(&self) -> bool {
        for (_id, stream) in self.stream_map.iter() {
            match stream.state {
                H2StreamState::Closed(_) => {}
                _ => {
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn stream_state(&self, id: StreamId) -> Option<H2StreamState> {
        self.stream_map.get(&id).map(|stream| stream.state)
    }

    pub(crate) fn insert(&mut self, id: StreamId, headers: Frame, data: BodyDataRef) {
        let send_window = SendWindow::new(self.stream_send_window_size as i32);
        let recv_window = RecvWindow::new(self.stream_recv_window_size as i32);
        let stream = Stream::new(recv_window, send_window, headers, data);
        self.stream_map.insert(id, stream);
    }

    pub(crate) fn push_back_pending_send(&mut self, id: StreamId) {
        self.pending_send.push_back(id);
    }

    pub(crate) fn push_pending_concurrency(&mut self, id: StreamId) {
        self.pending_concurrency.push_back(id);
    }

    pub(crate) fn is_pending_concurrency_empty(&self) -> bool {
        self.pending_concurrency.is_empty()
    }

    pub(crate) fn next_pending_stream(&mut self) -> Option<StreamId> {
        self.pending_send.pop_front()
    }

    pub(crate) fn pending_stream_num(&self) -> usize {
        self.pending_send.len()
    }

    pub(crate) fn try_consume_pending_concurrency(&mut self) {
        while !self.reach_max_concurrency() {
            match self.pending_concurrency.pop_front() {
                None => {
                    return;
                }
                Some(id) => {
                    self.increase_current_concurrency();
                    self.push_back_pending_send(id);
                }
            }
        }
    }

    pub(crate) fn increase_conn_send_window(&mut self, size: u32) -> Result<(), H2Error> {
        self.flow_control.increase_send_size(size)
    }

    pub(crate) fn reassign_conn_send_window(&mut self) {
        // Since the data structure of the body is a stream,
        // the size of a body cannot be obtained,
        // so all streams in pending_conn_window are added to the pending_send queue
        // again.
        loop {
            match self.pending_conn_window.pop_front() {
                None => break,
                Some(id) => {
                    self.push_back_pending_send(id);
                }
            }
        }
    }

    pub(crate) fn reassign_stream_send_window(
        &mut self,
        id: StreamId,
        size: u32,
    ) -> Result<(), H2Error> {
        if let Some(stream) = self.stream_map.get_mut(&id) {
            stream.send_window.increase_size(size)?;
        }
        if self.pending_stream_window.take(&id).is_some() {
            self.pending_send.push_back(id);
        }
        Ok(())
    }

    pub(crate) fn window_update_conn(
        &mut self,
        sender: &UnboundedSender<Frame>,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(window_update) = self.flow_control.check_conn_recv_window_update() {
            sender
                .send(window_update)
                .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
        }
        Ok(())
    }

    pub(crate) fn window_update_streams(
        &mut self,
        sender: &UnboundedSender<Frame>,
    ) -> Result<(), DispatchErrorKind> {
        loop {
            match self.window_updating_streams.pop_front() {
                None => return Ok(()),
                Some(id) => {
                    if let Some(stream) = self.stream_map.get_mut(&id) {
                        if !stream.is_init_or_active_flow_control() {
                            return Ok(());
                        }
                        if let Some(window_update) = stream.recv_window.check_window_update(id) {
                            sender
                                .send(window_update)
                                .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn headers(&mut self, id: StreamId) -> Result<Option<Frame>, H2Error> {
        match self.stream_map.get_mut(&id) {
            None => Err(H2Error::ConnectionError(ErrorCode::IntervalError)),
            Some(stream) => match stream.state {
                H2StreamState::Closed(_) => Ok(None),
                _ => Ok(stream.header.take()),
            },
        }
    }

    pub(crate) fn poll_read_body(
        &mut self,
        cx: &mut Context<'_>,
        id: StreamId,
    ) -> Result<DataReadState, H2Error> {
        // TODO Since the Array length needs to be a constant,
        // the minimum value is used here, which can be optimized to the MAX_FRAME_SIZE
        // updated in SETTINGS
        const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024;

        match self.stream_map.get_mut(&id) {
            None => Err(H2Error::ConnectionError(ErrorCode::IntervalError)),
            Some(stream) => match stream.state {
                H2StreamState::Closed(_) => Ok(DataReadState::Closed),
                _ => {
                    let stream_send_vacant = stream.send_window.size_available() as usize;
                    if stream_send_vacant == 0 {
                        self.pending_stream_window.insert(id);
                        return Ok(DataReadState::Pending);
                    }
                    let conn_send_vacant = self.flow_control.send_size_available();
                    if conn_send_vacant == 0 {
                        self.pending_conn_window.push_back(id);
                        return Ok(DataReadState::Pending);
                    }

                    let available = min(stream_send_vacant, conn_send_vacant);
                    let len = min(available, DEFAULT_MAX_FRAME_SIZE);

                    let mut buf = [0u8; DEFAULT_MAX_FRAME_SIZE];
                    self.poll_sized_data(cx, id, &mut buf[..len])
                }
            },
        }
    }

    fn poll_sized_data(
        &mut self,
        cx: &mut Context<'_>,
        id: StreamId,
        buf: &mut [u8],
    ) -> Result<DataReadState, H2Error> {
        let stream = if let Some(stream) = self.stream_map.get_mut(&id) {
            stream
        } else {
            return Err(H2Error::ConnectionError(ErrorCode::IntervalError));
        };
        match stream.data.poll_read(cx, buf) {
            Poll::Ready(Some(size)) => {
                if size > 0 {
                    stream.send_window.send_data(size as u32);
                    self.flow_control.send_data(size as u32);
                    let data_vec = Vec::from(&buf[..size]);
                    let flag = FrameFlags::new(0);

                    Ok(DataReadState::Ready(Frame::new(
                        id,
                        flag,
                        Payload::Data(Data::new(data_vec)),
                    )))
                } else {
                    let data_vec = vec![];
                    let mut flag = FrameFlags::new(1);
                    flag.set_end_stream(true);
                    Ok(DataReadState::Finish(Frame::new(
                        id,
                        flag,
                        Payload::Data(Data::new(data_vec)),
                    )))
                }
            }
            Poll::Ready(None) => Err(H2Error::ConnectionError(ErrorCode::IntervalError)),
            Poll::Pending => {
                self.push_back_pending_send(id);
                Ok(DataReadState::Pending)
            }
        }
    }

    pub(crate) fn get_go_away_streams(&mut self, last_stream_id: StreamId) -> Vec<StreamId> {
        let mut ids = vec![];
        for (id, unsent_stream) in self.stream_map.iter_mut() {
            if *id >= last_stream_id {
                match unsent_stream.state {
                    // TODO Whether the close state needs to be selected.
                    H2StreamState::Closed(_) => {}
                    H2StreamState::Idle => {
                        unsent_stream.state = H2StreamState::Closed(CloseReason::RemoteGoAway);
                        unsent_stream.header = None;
                        unsent_stream.data.clear();
                    }
                    _ => {
                        self.current_concurrent_streams -= 1;
                        unsent_stream.state = H2StreamState::Closed(CloseReason::RemoteGoAway);
                        unsent_stream.header = None;
                        unsent_stream.data.clear();
                    }
                };
                ids.push(*id);
            }
        }
        ids
    }

    pub(crate) fn get_all_unclosed_streams(&mut self) -> Vec<StreamId> {
        let mut ids = vec![];
        for (id, stream) in self.stream_map.iter_mut() {
            match stream.state {
                H2StreamState::Closed(_) => {}
                _ => {
                    stream.header = None;
                    stream.data.clear();
                    stream.state = H2StreamState::Closed(CloseReason::LocalGoAway);
                    ids.push(*id);
                }
            }
        }
        ids
    }

    pub(crate) fn clear_streams_states(&mut self) {
        self.window_updating_streams.clear();
        self.pending_stream_window.clear();
        self.pending_send.clear();
        self.pending_conn_window.clear();
        self.pending_concurrency.clear();
    }

    pub(crate) fn send_local_reset(&mut self, id: StreamId) -> StreamEndState {
        return match self.stream_map.get_mut(&id) {
            None => StreamEndState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match stream.state {
                H2StreamState::Closed(
                    CloseReason::LocalRst
                    | CloseReason::LocalGoAway
                    | CloseReason::RemoteRst
                    | CloseReason::RemoteGoAway,
                ) => StreamEndState::Ignore,
                H2StreamState::Closed(CloseReason::EndStream) => {
                    stream.state = H2StreamState::Closed(CloseReason::LocalRst);
                    StreamEndState::Ignore
                }
                _ => {
                    stream.state = H2StreamState::Closed(CloseReason::LocalRst);
                    stream.header = None;
                    stream.data.clear();
                    self.decrease_current_concurrency();
                    StreamEndState::OK
                }
            },
        };
    }

    pub(crate) fn send_headers_frame(&mut self, id: StreamId, eos: bool) -> FrameRecvState {
        match self.stream_map.get_mut(&id) {
            None => return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match &stream.state {
                H2StreamState::Idle => {
                    stream.state = if eos {
                        H2StreamState::LocalHalfClosed(ActiveState::WaitHeaders)
                    } else {
                        H2StreamState::Open {
                            send: ActiveState::WaitData,
                            recv: ActiveState::WaitHeaders,
                        }
                    };
                }
                H2StreamState::Open {
                    send: ActiveState::WaitHeaders,
                    recv,
                } => {
                    stream.state = if eos {
                        H2StreamState::LocalHalfClosed(*recv)
                    } else {
                        H2StreamState::Open {
                            send: ActiveState::WaitData,
                            recv: *recv,
                        }
                    };
                }
                H2StreamState::RemoteHalfClosed(ActiveState::WaitHeaders) => {
                    stream.state = if eos {
                        self.current_concurrent_streams -= 1;
                        H2StreamState::Closed(CloseReason::EndStream)
                    } else {
                        H2StreamState::RemoteHalfClosed(ActiveState::WaitData)
                    };
                }
                _ => {
                    return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                }
            },
        }
        FrameRecvState::OK
    }

    pub(crate) fn send_data_frame(&mut self, id: StreamId, eos: bool) -> FrameRecvState {
        match self.stream_map.get_mut(&id) {
            None => return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match &stream.state {
                H2StreamState::Open {
                    send: ActiveState::WaitData,
                    recv,
                } => {
                    if eos {
                        stream.state = H2StreamState::LocalHalfClosed(*recv);
                    }
                }
                H2StreamState::RemoteHalfClosed(ActiveState::WaitData) => {
                    if eos {
                        self.current_concurrent_streams -= 1;
                        stream.state = H2StreamState::Closed(CloseReason::EndStream);
                    }
                }
                _ => {
                    return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                }
            },
        }
        FrameRecvState::OK
    }

    pub(crate) fn recv_remote_reset(&mut self, id: StreamId) -> StreamEndState {
        if id > self.max_recv_id {
            return StreamEndState::Ignore;
        }
        return match self.stream_map.get_mut(&id) {
            None => StreamEndState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match stream.state {
                H2StreamState::Closed(..) => StreamEndState::Ignore,
                _ => {
                    stream.state = H2StreamState::Closed(CloseReason::RemoteRst);
                    stream.header = None;
                    stream.data.clear();
                    self.decrease_current_concurrency();
                    StreamEndState::OK
                }
            },
        };
    }

    pub(crate) fn recv_headers(&mut self, id: StreamId, eos: bool) -> FrameRecvState {
        if id > self.max_recv_id {
            return FrameRecvState::Ignore;
        }

        match self.stream_map.get_mut(&id) {
            None => return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match &stream.state {
                H2StreamState::Idle => {
                    change_stream_state!(Idle: eos, stream.state);
                }
                H2StreamState::ReservedRemote => {
                    change_stream_state!(HalfClosed: eos, stream.state);
                    if eos {
                        self.decrease_current_concurrency();
                    }
                }
                H2StreamState::Open {
                    send,
                    recv: ActiveState::WaitHeaders,
                } => {
                    change_stream_state!(Open: eos, stream.state, send);
                }
                H2StreamState::LocalHalfClosed(ActiveState::WaitHeaders) => {
                    change_stream_state!(HalfClosed: eos, stream.state);
                    if eos {
                        self.decrease_current_concurrency();
                    }
                }
                H2StreamState::Closed(CloseReason::LocalGoAway | CloseReason::LocalRst) => {
                    return FrameRecvState::Ignore;
                }
                _ => {
                    return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                }
            },
        }
        FrameRecvState::OK
    }

    pub(crate) fn recv_data(&mut self, id: StreamId, eos: bool) -> FrameRecvState {
        if id > self.max_recv_id {
            return FrameRecvState::Ignore;
        }
        match self.stream_map.get_mut(&id) {
            None => return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            Some(stream) => match &stream.state {
                H2StreamState::Open {
                    send,
                    recv: ActiveState::WaitData,
                } => {
                    if eos {
                        stream.state = H2StreamState::RemoteHalfClosed(*send);
                    }
                }
                H2StreamState::LocalHalfClosed(ActiveState::WaitData) => {
                    if eos {
                        stream.state = H2StreamState::Closed(CloseReason::EndStream);
                        self.decrease_current_concurrency();
                    }
                }
                H2StreamState::Closed(CloseReason::LocalGoAway | CloseReason::LocalRst) => {
                    return FrameRecvState::Ignore;
                }
                _ => {
                    return FrameRecvState::Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                }
            },
        }
        FrameRecvState::OK
    }

    pub(crate) fn generate_id(&mut self) -> Result<StreamId, DispatchErrorKind> {
        let id = self.next_stream_id;
        if self.next_stream_id < DEFAULT_MAX_STREAM_ID {
            self.next_stream_id += 2;
            Ok(id)
        } else {
            Err(DispatchErrorKind::H2(H2Error::ConnectionError(
                ErrorCode::ProtocolError,
            )))
        }
    }
}

impl Stream {
    pub(crate) fn new(
        recv_window: RecvWindow,
        send_window: SendWindow,
        headers: Frame,
        data: BodyDataRef,
    ) -> Self {
        Self {
            recv_window,
            send_window,
            state: H2StreamState::Idle,
            header: Some(headers),
            data,
        }
    }

    pub(crate) fn is_init_or_active_flow_control(&self) -> bool {
        matches!(
            self.state,
            H2StreamState::Idle
                | H2StreamState::Open {
                    recv: ActiveState::WaitData,
                    ..
                }
                | H2StreamState::LocalHalfClosed(ActiveState::WaitData)
        )
    }
}
