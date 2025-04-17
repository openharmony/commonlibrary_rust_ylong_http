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

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use ylong_http::h3::{Data, Frame, H3Error, H3ErrorCode, Payload, DATA_FRAME_TYPE};

use crate::runtime::{BoundedSender, SendError};
use crate::util::data_ref::BodyDataRef;
use crate::util::dispatcher::http3::{DispatchErrorKind, RespMessage};

pub(crate) type OutputSendFut =
    Pin<Box<dyn Future<Output = Result<(), SendError<RespMessage>>> + Send + Sync>>;

const HTTP3_FIRST_BIDI_STREAM_ID: u64 = 0u64;
const HTTP3_FIRST_UNI_STREAM_ID: u64 = 2u64;
const HTTP3_MAX_STREAM_ID: u64 = (1u64 << 62) - 1;
const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 100;

#[derive(PartialEq, Clone)]
pub(crate) enum H3StreamState {
    Sending,
    HeadersReceived,
    BodyReceived,
    TrailerReceived,
    Shutdown,
}

#[derive(PartialEq, Clone)]
pub(crate) enum QUICStreamType {
    ClientInitialBidirectional,
    ServerInitialBidirectional,
    ClientInitialUnidirectional,
    ServerInitialUnidirectional,
}

impl QUICStreamType {
    pub(crate) fn from(id: u64) -> Self {
        match id % 4 {
            0 => QUICStreamType::ClientInitialBidirectional,
            1 => QUICStreamType::ServerInitialBidirectional,
            2 => QUICStreamType::ClientInitialUnidirectional,
            _ => QUICStreamType::ServerInitialUnidirectional,
        }
    }
}

// Unidirectional Streams
pub(crate) struct BidirectionalStream {
    pub(crate) state: H3StreamState,
    pub(crate) frame_tx: BoundedSender<RespMessage>,
    pub(crate) header: Option<Frame>,
    pub(crate) data: BodyDataRef,
    pub(crate) pending_message: VecDeque<RespMessage>,
    pub(crate) encoding: bool,
    pub(crate) curr_message: Option<OutputSendFut>,
}

impl BidirectionalStream {
    fn new(frame_tx: BoundedSender<RespMessage>, header: Frame, data: BodyDataRef) -> Self {
        Self {
            state: H3StreamState::Sending,
            frame_tx,
            header: Some(header),
            data,
            pending_message: VecDeque::new(),
            encoding: false,
            curr_message: None,
        }
    }

    fn transmit_message(
        &mut self,
        cx: &mut Context<'_>,
        message: RespMessage,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        let mut task = {
            let sender = self.frame_tx.clone();
            let ft = async move { sender.send(message).await };
            Box::pin(ft)
        };

        match task.as_mut().poll(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            // The current coroutine sending the request exited prematurely.
            Poll::Ready(Err(_)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Pending => {
                self.curr_message = Some(task);
                Poll::Pending
            }
        }
    }
}

pub(crate) struct Streams {
    bidirectional_stream: HashMap<u64, BidirectionalStream>,
    control_stream_id: Option<u64>,
    peer_control_stream_id: Option<u64>,
    qpack_encode_stream_id: Option<u64>,
    qpack_decode_stream_id: Option<u64>,
    peer_qpack_encode_stream_id: Option<u64>,
    peer_qpack_decode_stream_id: Option<u64>,
    // unused now
    goaway_id: Option<u64>,
    peer_goaway_id: Option<u64>,
    // meet the sending conditions, waiting for sending
    pending_send: VecDeque<u64>,
    // cannot recv cause of stream blocks
    pending_recv: HashSet<u64>,
    // stream resumes and should decode again
    resume_recv: VecDeque<u64>,
    // too many working streams, pending for concurrency
    pending_concurrency: VecDeque<u64>,
    // cannot recv cause of channel blocked
    pending_channel: HashSet<u64>,
    working_stream_num: u32,
    max_stream_concurrency: u32,
    next_uni_stream_id: AtomicU64,
    next_bidi_stream_id: AtomicU64,
}

impl Streams {
    pub(crate) fn new() -> Self {
        Self {
            bidirectional_stream: HashMap::new(),
            control_stream_id: None,
            peer_control_stream_id: None,
            qpack_encode_stream_id: None,
            qpack_decode_stream_id: None,
            peer_qpack_encode_stream_id: None,
            peer_qpack_decode_stream_id: None,
            goaway_id: None,
            peer_goaway_id: None,
            pending_send: VecDeque::new(),
            pending_recv: HashSet::new(),
            resume_recv: VecDeque::new(),
            pending_concurrency: VecDeque::new(),
            pending_channel: HashSet::new(),
            working_stream_num: 0,
            max_stream_concurrency: DEFAULT_MAX_CONCURRENT_STREAMS,
            next_uni_stream_id: AtomicU64::new(HTTP3_FIRST_UNI_STREAM_ID),
            next_bidi_stream_id: AtomicU64::new(HTTP3_FIRST_BIDI_STREAM_ID),
        }
    }

    pub(crate) fn new_unidirectional_stream(
        &mut self,
        header: Frame,
        data: BodyDataRef,
        rx: BoundedSender<RespMessage>,
    ) -> Result<(), DispatchErrorKind> {
        let id =
            self.get_next_bidi_stream_id()
                .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3GeneralProtocolError,
                )))?;
        self.bidirectional_stream
            .insert(id, BidirectionalStream::new(rx, header, data));
        if self.reach_max_concurrency() {
            self.push_back_pending_concurrency(id);
        } else {
            self.push_back_pending_send(id);
            self.increase_current_concurrency();
        }
        Ok(())
    }

    pub(crate) fn send_frame(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        frame: Frame,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            match stream.state {
                H3StreamState::Sending => {
                    if let Payload::Headers(_) = frame.payload() {
                        stream.state = H3StreamState::HeadersReceived;
                    } else {
                        return Err(DispatchErrorKind::H3(H3Error::Connection(
                            H3ErrorCode::H3FrameUnexpected,
                        )));
                    }
                }
                H3StreamState::HeadersReceived => {
                    if let Payload::Headers(_) = frame.payload() {
                        return Err(DispatchErrorKind::H3(H3Error::Connection(
                            H3ErrorCode::H3FrameUnexpected,
                        )));
                    } else {
                        stream.state = H3StreamState::BodyReceived;
                    }
                }
                H3StreamState::BodyReceived => {
                    if let Payload::Headers(_) = frame.payload() {
                        stream.state = H3StreamState::TrailerReceived;
                    }
                }
                H3StreamState::TrailerReceived => {
                    return Err(DispatchErrorKind::H3(H3Error::Connection(
                        H3ErrorCode::H3FrameUnexpected,
                    )));
                }
                H3StreamState::Shutdown => {
                    // stream has been shutdown, drop frame
                    return Ok(());
                }
            }
            if stream.curr_message.is_some() {
                stream.pending_message.push_back(RespMessage::Output(frame));
                return Ok(());
            }
            if let Poll::Ready(ret) = stream.transmit_message(cx, RespMessage::Output(frame)) {
                ret
            } else {
                self.stream_pend_channel(id);
                Ok(())
            }
        } else {
            Err(DispatchErrorKind::ChannelClosed)
        }
    }

    pub(crate) fn send_error(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        error: DispatchErrorKind,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            stream.pending_message.clear();
            if let Poll::Ready(ret) = stream.transmit_message(cx, RespMessage::OutputExit(error)) {
                ret
            } else {
                self.stream_pend_channel(id);
                Ok(())
            }
        } else {
            Err(DispatchErrorKind::ChannelClosed)
        }
    }

    pub(crate) fn control_stream_id(&mut self) -> Option<u64> {
        if self.control_stream_id.is_some() {
            self.control_stream_id
        } else {
            self.control_stream_id = self.get_next_uni_stream_id();
            self.control_stream_id
        }
    }

    pub(crate) fn qpack_decode_stream_id(&mut self) -> Option<u64> {
        if self.qpack_decode_stream_id.is_some() {
            self.qpack_decode_stream_id
        } else {
            self.qpack_decode_stream_id = self.get_next_uni_stream_id();
            self.qpack_decode_stream_id
        }
    }

    pub(crate) fn qpack_encode_stream_id(&mut self) -> Option<u64> {
        if self.qpack_encode_stream_id.is_some() {
            self.qpack_encode_stream_id
        } else {
            self.qpack_encode_stream_id = self.get_next_uni_stream_id();
            self.qpack_encode_stream_id
        }
    }

    pub(crate) fn peer_qpack_encode_stream_id(&self) -> Option<u64> {
        self.peer_qpack_encode_stream_id
    }

    pub(crate) fn peer_goaway_id(&self) -> Option<u64> {
        self.peer_goaway_id
    }

    #[allow(unused)]
    pub(crate) fn goaway_id(&self) -> Option<u64> {
        self.goaway_id
    }

    pub(crate) fn peer_control_stream_id(&self) -> Option<u64> {
        self.peer_control_stream_id
    }

    pub(crate) fn peer_qpack_decode_stream_id(&self) -> Option<u64> {
        self.peer_qpack_decode_stream_id
    }

    pub(crate) fn set_peer_qpack_encode_stream_id(
        &mut self,
        id: u64,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(old_id) = self.peer_qpack_encode_stream_id {
            if old_id != id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3StreamCreationError,
                )));
            }
        } else {
            self.peer_qpack_encode_stream_id = Some(id);
        }
        Ok(())
    }

    pub(crate) fn set_peer_control_stream_id(&mut self, id: u64) -> Result<(), DispatchErrorKind> {
        if let Some(old_id) = self.peer_control_stream_id {
            if old_id != id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3StreamCreationError,
                )));
            }
        } else {
            self.peer_control_stream_id = Some(id);
        }
        Ok(())
    }

    pub(crate) fn set_peer_qpack_decode_stream_id(
        &mut self,
        id: u64,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(old_id) = self.peer_qpack_decode_stream_id {
            if old_id != id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3StreamCreationError,
                )));
            }
        } else {
            self.peer_qpack_decode_stream_id = Some(id);
        }
        Ok(())
    }

    #[allow(unused)]
    pub(crate) fn set_goaway_id(&mut self, id: u64) -> Result<(), DispatchErrorKind> {
        if let Some(old_goaway_id) = self.goaway_id {
            if id > old_goaway_id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3InternalError,
                )));
            }
        }
        self.goaway_id = Some(id);
        Ok(())
    }

    pub(crate) fn get_header(&mut self, id: u64) -> Result<Option<Frame>, DispatchErrorKind> {
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            Ok(stream.header.take())
        } else {
            Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            )))
        }
    }

    pub(crate) fn frame_acceptable(&mut self, id: u64) -> bool {
        !self.is_stream_recv_pending(id) && !self.is_stream_channel_pending(id)
    }

    pub(crate) fn decrease_current_concurrency(&mut self) {
        self.working_stream_num -= 1;
    }

    pub(crate) fn increase_current_concurrency(&mut self) {
        self.working_stream_num += 1;
    }

    pub(crate) fn current_concurrency(&mut self) -> u32 {
        self.working_stream_num
    }

    pub(crate) fn reach_max_concurrency(&mut self) -> bool {
        self.working_stream_num >= self.max_stream_concurrency
    }

    pub(crate) fn push_back_pending_send(&mut self, id: u64) {
        self.pending_send.push_back(id);
    }

    pub(crate) fn next_stream(&mut self) -> Option<u64> {
        self.pending_send.pop_front()
    }

    pub(crate) fn pending_stream_len(&mut self) -> u64 {
        self.pending_send.len() as u64
    }

    pub(crate) fn push_back_pending_concurrency(&mut self, id: u64) {
        self.pending_concurrency.push_back(id);
    }

    pub(crate) fn pop_front_pending_concurrency(&mut self) -> Option<u64> {
        self.pending_concurrency.pop_front()
    }

    pub(crate) fn stream_pend_channel(&mut self, id: u64) {
        self.pending_channel.insert(id);
    }

    pub(crate) fn is_stream_channel_pending(&self, id: u64) -> bool {
        self.pending_channel.contains(&id)
    }

    pub(crate) fn try_consume_pending_concurrency(&mut self) {
        while !self.reach_max_concurrency() {
            match self.pop_front_pending_concurrency() {
                Some(id) => {
                    self.push_back_pending_send(id);
                    self.increase_current_concurrency();
                }
                None => {
                    return;
                }
            }
        }
    }

    pub(crate) fn get_next_uni_stream_id(&self) -> Option<u64> {
        let id = self.next_uni_stream_id.fetch_add(4, Ordering::Relaxed);
        if id > HTTP3_MAX_STREAM_ID {
            None
        } else {
            Some(id)
        }
    }

    pub(crate) fn get_next_bidi_stream_id(&self) -> Option<u64> {
        let id = self.next_bidi_stream_id.fetch_add(4, Ordering::Relaxed);
        if id > HTTP3_MAX_STREAM_ID {
            None
        } else {
            Some(id)
        }
    }

    pub(crate) fn pend_stream_recv(&mut self, id: u64) {
        self.pending_recv.insert(id);
    }

    pub(crate) fn resume_stream_recv(&mut self, id: u64) {
        self.pending_recv.remove(&id);
        self.resume_recv.push_back(id);
    }

    pub(crate) fn is_stream_recv_pending(&self, id: u64) -> bool {
        self.pending_recv.contains(&id)
    }

    pub(crate) fn get_resume_stream_id(&mut self) -> Option<u64> {
        self.resume_recv.pop_front()
    }

    pub(crate) fn poll_sized_data(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
        buf: &mut [u8],
    ) -> Result<DataReadState, DispatchErrorKind> {
        let stream = self
            .bidirectional_stream
            .get_mut(&id)
            .ok_or(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            )))?;

        if stream.state == H3StreamState::Shutdown {
            return Ok(DataReadState::Closed);
        }

        match stream.data.poll_read(cx, buf) {
            Poll::Ready(Ok(size)) => {
                if size > 0 {
                    let data_vec = Vec::from(&buf[..size]);
                    Ok(DataReadState::Ready(Box::new(Frame::new(
                        DATA_FRAME_TYPE,
                        Payload::Data(Data::new(data_vec)),
                    ))))
                } else {
                    Ok(DataReadState::Finish)
                }
            }
            Poll::Ready(Err(_)) => Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            ))),
            Poll::Pending => {
                self.push_back_pending_send(id);
                Ok(DataReadState::Pending)
            }
        }
    }

    pub(crate) fn shutdown_stream(&mut self, cx: &mut Context<'_>, id: u64, err: &H3ErrorCode) {
        let Some(stream) = self.bidirectional_stream.get_mut(&id) else {
            return;
        };
        if stream
            .transmit_message(
                cx,
                RespMessage::OutputExit(DispatchErrorKind::H3(H3Error::Stream(id, *err))),
            )
            .is_pending()
        {
            self.stream_pend_channel(id);
        }
        self.decrease_current_concurrency();
        // stream.header = None;
        // stream.pending_frame.clear();
        // stream.data.clear();
        // stream.state = H3StreamState::Shutdown;
    }

    pub(crate) fn goaway(
        &mut self,
        cx: &mut Context<'_>,
        goaway_id: u64,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(old_goaway_id) = self.peer_goaway_id() {
            if goaway_id > old_goaway_id {
                return Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3IdError,
                )));
            }
        }
        if QUICStreamType::from(goaway_id) != QUICStreamType::ClientInitialBidirectional {
            return Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3IdError,
            )));
        }
        self.goaway_id = Some(goaway_id);
        let mut pending_channels = Vec::new();
        for (id, stream) in self.bidirectional_stream.iter_mut() {
            if id > &goaway_id {
                stream.state = H3StreamState::Shutdown;
                stream.header = None;
                stream.pending_message.clear();
                stream.data.clear();
                if stream
                    .transmit_message(
                        cx,
                        RespMessage::OutputExit(DispatchErrorKind::GoawayReceived),
                    )
                    .is_pending()
                {
                    pending_channels.push(*id);
                }
            }
        }
        for id in pending_channels {
            self.stream_pend_channel(id);
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self, cx: &mut Context<'_>, err: &DispatchErrorKind) {
        let mut pending_channels = Vec::new();
        for (id, stream) in self.bidirectional_stream.iter_mut() {
            stream.state = H3StreamState::Shutdown;
            stream.header = None;
            stream.pending_message.clear();
            stream.data.clear();
            if stream
                .transmit_message(cx, RespMessage::OutputExit(err.clone()))
                .is_pending()
            {
                pending_channels.push(*id);
            }
        }
        for id in pending_channels {
            self.stream_pend_channel(id);
        }
    }

    pub(crate) fn set_encoding(
        &mut self,
        id: u64,
        encoding: bool,
    ) -> Result<(), DispatchErrorKind> {
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            stream.encoding = encoding;
            Ok(())
        } else {
            Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            )))
        }
    }

    pub(crate) fn encoding(&mut self, id: u64) -> Result<bool, DispatchErrorKind> {
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            Ok(stream.encoding)
        } else {
            Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            )))
        }
    }

    pub(crate) fn finish_stream(
        &mut self,
        cx: &mut Context<'_>,
        id: u64,
    ) -> Result<(), DispatchErrorKind> {
        if QUICStreamType::from(id) != QUICStreamType::ClientInitialBidirectional {
            return if Some(id) == self.peer_control_stream_id()
                || Some(id) == self.peer_qpack_encode_stream_id()
                || Some(id) == self.peer_qpack_decode_stream_id()
            {
                Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3ClosedCriticalStream,
                )))
            } else {
                Ok(())
            };
        }
        self.decrease_current_concurrency();
        if let Some(stream) = self.bidirectional_stream.get_mut(&id) {
            stream.state = H3StreamState::Shutdown;
            if stream.curr_message.is_none() {
                if let Poll::Ready(ret) = stream.transmit_message(
                    cx,
                    RespMessage::OutputExit(DispatchErrorKind::StreamFinished),
                ) {
                    ret
                } else {
                    self.stream_pend_channel(id);
                    Ok(())
                }
            } else {
                stream
                    .pending_message
                    .push_back(RespMessage::OutputExit(DispatchErrorKind::StreamFinished));
                Ok(())
            }
        } else {
            Err(DispatchErrorKind::H3(H3Error::Connection(
                H3ErrorCode::H3InternalError,
            )))
        }
    }

    pub(crate) fn poll_blocked_message(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        let mut new_set = HashSet::new();
        for id in &self.pending_channel {
            let Some(stream) = self.bidirectional_stream.get_mut(id) else {
                return Poll::Ready(Err(DispatchErrorKind::H3(H3Error::Connection(
                    H3ErrorCode::H3InternalError,
                ))));
            };
            if let Some(mut task) = stream.curr_message.take() {
                match task.as_mut().poll(cx) {
                    Poll::Ready(Ok(_)) => {}
                    Poll::Ready(Err(_)) => {
                        // todo: shutdown
                        stream.state = H3StreamState::Shutdown;
                    }
                    Poll::Pending => {
                        stream.curr_message = Some(task);
                        new_set.insert(*id);
                        continue;
                    }
                }
            }
            while let Some(message) = stream.pending_message.pop_front() {
                match stream.transmit_message(cx, message) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Pending => {
                        new_set.insert(*id);
                        break;
                    }
                    Poll::Ready(Err(_)) => {
                        stream.state = H3StreamState::Shutdown;
                        break;
                    }
                }
            }
        }
        self.pending_channel = new_set;
        Poll::Pending
    }
}

pub(crate) enum DataReadState {
    Closed,
    // Wait for poll_read or wait for window.
    Pending,
    Ready(Box<Frame>),
    Finish,
}
