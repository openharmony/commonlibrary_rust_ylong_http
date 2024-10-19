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

use std::collections::HashMap;
use std::mem::take;

use crate::h3::error::CommonError::FieldMissing;
use crate::h3::error::DecodeError::{FrameSizeError, UnexpectedFrame, UnsupportedSetting};
use crate::h3::error::{CommonError, DecodeError, H3Error};
use crate::h3::frame::{
    CancelPush, Data, GoAway, Headers, MaxPushId, Payload, PushPromise, Settings, DATA_FRAME_TYPE,
    HEADERS_FRAME_TYPE, PUSH_PROMISE_FRAME_TYPE, SETTINGS_FRAME_TYPE,
};
use crate::h3::octets::{ReadableBytes, WritableBytes};
use crate::h3::parts::Parts;
use crate::h3::qpack::error::QpackError;
use crate::h3::qpack::table::DynamicTable;
use crate::h3::qpack::{FieldDecodeState, FiledLines, QpackDecoder};
use crate::h3::stream::StreamMessage::Request;
use crate::h3::stream::{FrameKind, Frames, StreamMessage};
use crate::h3::{frame, is_bidirectional, stream, Frame, H3ErrorCode};

/// HTTP3 stream bytes sequence decoder.
/// The http3 stream decoder deserializes stream data into readable structured
/// data, including stream type, Frame, etc.
///
/// # Examples
///
/// ```
/// use ylong_http::h3::FrameDecoder;
///
/// let mut decoder = FrameDecoder::new(100, 10240);
/// let data_frame_bytes = &[0, 5, b'h', b'e', b'l', b'l', b'o'];
/// let message = decoder.decode(0, data_frame_bytes).unwrap();
/// ```
pub struct FrameDecoder {
    qpack_decoder: QpackDecoder,
    streams: HashMap<u64, DecodedH3Stream>,
}

#[derive(Copy, Clone)]
enum DecodeState {
    StreamType,
    PushId,
    FrameType,
    PayloadLen,
    HeadersPayload,
    DataPayload,
    SettingsPayload,
    VariablePayload,
    PushPromisePayload,
    UnknownPayload,
    QpackDecoderInst,
    QpackEncoderInst,
    DropUnknown,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum StreamType {
    Init,
    Request,
    Control,
    Push,
    QpackEncoder,
    QpackDecoder,
    Unknown,
}

struct DecodedH3Stream {
    ty: StreamType,
    state: DecodeState,
    buffer: Vec<u8>,
    offset: usize,
    push_id: Option<u64>,
    frame_type: Option<u64>,
    push_frame_id: Option<u64>,
    payload_len: Option<u64>,
    stream_set: bool,
}

enum DecodePartRes {
    ReturnOuter,
    Continue,
}

impl FrameDecoder {
    /// `FrameDecoder` constructor. max_blocked_streams is the maximum number of
    /// stream blocks allowed by qpack, and max_table_capacity is the
    /// maximum dynamic table capacity allowed by the encoder.
    pub fn new(max_blocked_streams: usize, max_table_capacity: usize) -> Self {
        Self {
            qpack_decoder: QpackDecoder::new(max_blocked_streams, max_table_capacity),
            streams: HashMap::new(),
        }
    }

    /// Sets allowed_max_field_section_size Setting. Only one call is allowed,
    /// and the max_field_section_size needs to be sent to the peer through the
    /// Settings frame
    pub fn local_allowed_max_field_section_size(&mut self, size: usize) {
        self.qpack_decoder.set_max_field_section_size(size)
    }

    /// The Decoder sends the Stream Cancellation instruction to actively cancel
    /// the stream.
    pub fn cancel_stream(&mut self, stream_id: u64, buf: &mut [u8]) -> Result<usize, H3Error> {
        self.streams.remove(&stream_id);
        self.qpack_decoder
            .stream_cancel(stream_id, buf)
            .map_err(|e| DecodeError::QpackError(e).into())
    }

    /// Cleans the stream information when the stream normally ends.
    pub fn finish_stream(&mut self, id: u64) -> Result<(), H3Error> {
        if is_bidirectional(id) {
            self.qpack_decoder
                .finish_stream(id)
                .map_err(|e| H3Error::Decode(e.into()))?;
        }
        self.streams.remove(&id);
        Ok(())
    }

    /// Deserializes stream data into readable structured data,
    /// including stream type, Frame, etc.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h3::FrameDecoder;
    ///
    /// let mut decoder = FrameDecoder::new(100, 10240);
    /// let data_frame_bytes = &[0, 5, b'h', b'e', b'l', b'l', b'o'];
    /// let message = decoder.decode(0, data_frame_bytes).unwrap();
    /// ```
    pub fn decode(&mut self, id: u64, src: &[u8]) -> Result<StreamMessage, H3Error> {
        let mut stream = if let Some(stream) = self.streams.remove(&id) {
            stream
        } else {
            DecodedH3Stream::new(id)
        };
        stream.buffer.extend_from_slice(src);
        let mut frames = Frames::new();
        loop {
            match stream.decode_state() {
                DecodeState::StreamType => {
                    if let DecodePartRes::ReturnOuter = stream.decode_stream_type()? {
                        self.streams.insert(id, stream);
                        return Ok(StreamMessage::WaitingMore);
                    }
                }
                DecodeState::PushId => {
                    if let DecodePartRes::ReturnOuter = stream.decode_push_id()? {
                        self.streams.insert(id, stream);
                        return Ok(StreamMessage::WaitingMore);
                    }
                }
                // The StreamType branch ensures that only Request/Control/Push can go to the
                // FrameType branch.
                DecodeState::FrameType => {
                    if let DecodePartRes::ReturnOuter = stream.decode_frame_type(&mut frames)? {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::PayloadLen => {
                    if let DecodePartRes::ReturnOuter = stream.decode_payload_len(&mut frames)? {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::DataPayload => {
                    if let DecodePartRes::ReturnOuter = stream.decode_data_payload(&mut frames)? {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::HeadersPayload => {
                    if let DecodePartRes::ReturnOuter =
                        stream.decode_headers_payload(&mut frames, &mut self.qpack_decoder, id)?
                    {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::VariablePayload => {
                    if let DecodePartRes::ReturnOuter =
                        stream.decode_variable_payload(&mut frames)?
                    {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::SettingsPayload => {
                    if let DecodePartRes::ReturnOuter =
                        stream.decode_settings_payload(&mut frames)?
                    {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::PushPromisePayload => {
                    if let DecodePartRes::ReturnOuter = stream.decode_push_payload(&mut frames)? {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::UnknownPayload => {
                    if let DecodePartRes::ReturnOuter =
                        stream.decode_unknown_payload(&mut frames)?
                    {
                        let message = stream.return_by_type(frames)?;
                        self.streams.insert(id, stream);
                        return Ok(message);
                    }
                }
                DecodeState::QpackDecoderInst => {
                    let reader = ReadableBytes::from(&stream.buffer.as_slice()[stream.offset..]);
                    let inst = Vec::from(reader.remaining());
                    stream.clear_buffer();
                    self.streams.insert(id, stream);
                    return Ok(StreamMessage::QpackDecoder(inst));
                }
                DecodeState::QpackEncoderInst => {
                    let reader = ReadableBytes::from(&stream.buffer.as_slice()[stream.offset..]);
                    let unblocked = self
                        .qpack_decoder
                        .decode_ins(reader.remaining())
                        .map_err(DecodeError::QpackError)?;
                    stream.clear_buffer();
                    self.streams.insert(id, stream);
                    return Ok(StreamMessage::QpackEncoder(unblocked));
                }
                DecodeState::DropUnknown => {
                    stream.clear_buffer();
                    return Ok(StreamMessage::Unknown);
                }
            }
        }
    }
}

impl StreamType {
    pub(crate) fn is_request(&self) -> bool {
        *self == StreamType::Request
    }

    pub(crate) fn is_control(&self) -> bool {
        *self == StreamType::Control
    }

    pub(crate) fn is_push(&self) -> bool {
        *self == StreamType::Push
    }
}

impl DecodedH3Stream {
    pub(crate) fn new(id: u64) -> Self {
        const DECODED_BUFFER_SIZE: usize = 1024;
        let (ty, state) = if is_bidirectional(id) {
            (StreamType::Request, DecodeState::FrameType)
        } else {
            (StreamType::Init, DecodeState::StreamType)
        };
        Self {
            ty,
            state,
            // TODO a property size.
            buffer: Vec::with_capacity(DECODED_BUFFER_SIZE),
            offset: 0,
            push_id: None,
            frame_type: None,
            push_frame_id: None,
            payload_len: None,
            stream_set: false,
        }
    }

    pub(crate) fn decode_state(&self) -> DecodeState {
        self.state
    }

    pub(crate) fn set_decode_state(&mut self, state: DecodeState) {
        self.state = state
    }

    fn init_state_by_type(&mut self) {
        let next_state = match self.ty {
            StreamType::Control => DecodeState::FrameType,
            StreamType::Push => DecodeState::PushId,
            StreamType::QpackEncoder => DecodeState::QpackEncoderInst,
            StreamType::QpackDecoder => DecodeState::QpackDecoderInst,
            StreamType::Unknown => DecodeState::DropUnknown,
            _ => unreachable!(),
        };
        self.set_decode_state(next_state);
    }

    fn stream_type(&self) -> StreamType {
        self.ty
    }

    fn curr_frame_type(&self) -> Option<u64> {
        self.frame_type
    }

    fn set_stream_type(&mut self, ty: StreamType) {
        self.ty = ty
    }

    fn is_set(&self) -> bool {
        self.stream_set
    }

    fn remain_payload_len(&self) -> u64 {
        self.payload_len.unwrap_or(0)
    }

    fn subtract_payload_len(&mut self, off: usize) -> Result<(), H3Error> {
        match self.payload_len {
            None => Err(CommonError::FieldMissing.into()),
            Some(curr) => {
                let (remain, overflow) = curr.overflowing_sub(off as u64);
                if overflow {
                    Err(CommonError::CalculateOverflow.into())
                } else {
                    self.payload_len = Some(remain);
                    Ok(())
                }
            }
        }
    }

    fn push_id(&self) -> Result<u64, H3Error> {
        self.push_id.ok_or(CommonError::FieldMissing.into())
    }

    fn clear_frame(&mut self) {
        self.frame_type = None;
        self.payload_len = None;
    }

    fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.offset = 0;
    }

    fn return_by_type(&self, frames: Frames) -> Result<StreamMessage, H3Error> {
        match self.stream_type() {
            StreamType::Request => Ok(StreamMessage::Request(frames)),
            StreamType::Push => {
                let push_id = self.push_id()?;
                Ok(StreamMessage::Push(push_id, frames))
            }
            StreamType::Control => Ok(StreamMessage::Control(frames)),
            _ => {
                // Note: unreachable
                Err(UnexpectedFrame(self.frame_type.unwrap()).into())
            }
        }
    }

    fn decode_stream_type(&mut self) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            Ok(integer) => {
                self.offset += reader.index();
                let ty = decode_type(integer);
                self.set_stream_type(ty);
                self.init_state_by_type();
                Ok(DecodePartRes::Continue)
            }
            // Byte shortage
            Err(_) => {
                self.set_decode_state(DecodeState::StreamType);
                Ok(DecodePartRes::ReturnOuter)
            }
        }
    }

    fn decode_push_id(&mut self) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            // TODO enable push
            Ok(integer) => {
                self.push_id = Some(integer);
                self.offset += reader.index();
                self.set_decode_state(DecodeState::FrameType);
                Ok(DecodePartRes::Continue)
            }
            // Byte shortage
            Err(_) => {
                self.set_decode_state(DecodeState::PushId);
                Ok(DecodePartRes::ReturnOuter)
            }
        }
    }

    fn decode_frame_type(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            Ok(integer) => {
                match integer {
                    frame::DATA_FRAME_TYPE | frame::HEADERS_FRAME_TYPE => {
                        if !(self.stream_type().is_request() || self.stream_type().is_push()) {
                            return Err(DecodeError::UnexpectedFrame(integer).into());
                        }
                    }
                    frame::PUSH_PROMISE_FRAME_TYPE => {
                        if !self.stream_type().is_request() {
                            return Err(DecodeError::UnexpectedFrame(integer).into());
                        }
                    }
                    frame::SETTINGS_FRAME_TYPE => {
                        if !self.stream_type().is_control() || self.is_set() {
                            return Err(DecodeError::UnexpectedFrame(integer).into());
                        }
                        self.stream_set = true;
                    }
                    frame::CANCEL_PUSH_FRAME_TYPE => {
                        if !self.stream_type().is_control() {
                            return Err(DecodeError::UnexpectedFrame(integer).into());
                        }
                    }
                    frame::MAX_PUSH_ID_FRAME_TYPE => {
                        return Err(DecodeError::UnexpectedFrame(integer).into())
                    }
                    _ => {}
                }
                self.frame_type = Some(integer);
                self.offset += reader.index();
                self.set_decode_state(DecodeState::PayloadLen);
                Ok(DecodePartRes::Continue)
            }
            // Byte shortage
            Err(_) => {
                self.set_decode_state(DecodeState::FrameType);
                frames.push(FrameKind::Partial);
                Ok(DecodePartRes::ReturnOuter)
            }
        }
    }

    fn decode_payload_len(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            Ok(integer) => {
                self.payload_len = Some(integer);
                self.offset += reader.index();
                match self.curr_frame_type() {
                    None => {
                        unreachable!()
                    }
                    Some(DATA_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::DataPayload);
                    }
                    Some(HEADERS_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::HeadersPayload);
                    }
                    Some(SETTINGS_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::SettingsPayload);
                    }
                    Some(PUSH_PROMISE_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::PushPromisePayload);
                    }
                    Some(frame::GOAWAY_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::VariablePayload);
                    }
                    Some(frame::MAX_PUSH_ID_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::VariablePayload);
                    }
                    Some(frame::CANCEL_PUSH_FRAME_TYPE) => {
                        self.set_decode_state(DecodeState::VariablePayload);
                    }
                    _ => {
                        self.set_decode_state(DecodeState::UnknownPayload);
                    }
                }
            }
            // Byte shortage
            Err(_) => {
                frames.push(FrameKind::Partial);
                return Ok(DecodePartRes::ReturnOuter);
            }
        }
        Ok(DecodePartRes::Continue)
    }

    fn decode_data_payload(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);

        // Note: None is impossible for `stream.payload_len`.
        let payload_len = self.remain_payload_len() as usize;
        if reader.cap() < payload_len {
            let frame = Frame::new(
                DATA_FRAME_TYPE,
                Payload::Data(Data::new(Vec::from(reader.remaining()))),
            );
            self.subtract_payload_len(reader.cap())?;
            self.clear_buffer();
            frames.push(FrameKind::Complete(Box::from(frame)));
            frames.push(FrameKind::Partial);
            Ok(DecodePartRes::ReturnOuter)
        } else {
            let frame = Frame::new(
                DATA_FRAME_TYPE,
                Payload::Data(Data::new(Vec::from(reader.slice(payload_len)?))),
            );
            frames.push(FrameKind::Complete(Box::from(frame)));
            self.offset += payload_len;
            let remaining = reader.cap();
            self.clear_frame();
            self.set_decode_state(DecodeState::FrameType);
            if remaining == 0 {
                self.clear_buffer();
                Ok(DecodePartRes::ReturnOuter)
            } else {
                Ok(DecodePartRes::Continue)
            }
        }
    }

    fn get_qpack_decoded_header(
        &mut self,
        frames: &mut Frames,
        qpack_decoder: &mut QpackDecoder,
        id: u64,
        remaining: usize,
    ) -> Result<DecodePartRes, H3Error> {
        let mut ins_buf = Vec::new();
        // TODO id can be u64.
        let (part, len) = qpack_decoder
            .finish(id, &mut ins_buf)
            .map_err(DecodeError::QpackError)?;
        let frame = match self.curr_frame_type() {
            Some(HEADERS_FRAME_TYPE) => {
                let mut headers_payload = Headers::new(part);
                if len.is_some() {
                    headers_payload.set_instruction(ins_buf);
                };
                Frame::new(HEADERS_FRAME_TYPE, Payload::Headers(headers_payload))
            }
            Some(PUSH_PROMISE_FRAME_TYPE) => {
                let mut push_promise =
                    PushPromise::new(self.push_frame_id.ok_or(FieldMissing)?, part);
                if len.is_some() {
                    push_promise.set_instruction(ins_buf)
                };
                Frame::new(PUSH_PROMISE_FRAME_TYPE, Payload::PushPromise(push_promise))
            }
            _ => unreachable!(),
        };
        frames.push(FrameKind::Complete(Box::from(frame)));
        self.clear_frame();
        self.set_decode_state(DecodeState::FrameType);
        if remaining == 0 {
            self.clear_buffer();
            Ok(DecodePartRes::ReturnOuter)
        } else {
            Ok(DecodePartRes::Continue)
        }
    }

    fn decode_headers_payload(
        &mut self,
        frames: &mut Frames,
        qpack_decoder: &mut QpackDecoder,
        id: u64,
    ) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        let payload_len = self.remain_payload_len() as usize;
        if reader.cap() < payload_len {
            if let FieldDecodeState::Blocked = qpack_decoder
                .decode_repr(reader.remaining(), id)
                .map_err(DecodeError::QpackError)?
            {
                frames.push(FrameKind::Blocked);
            } else {
                frames.push(FrameKind::Partial);
            }
            self.subtract_payload_len(reader.cap())?;
            self.clear_buffer();
            Ok(DecodePartRes::ReturnOuter)
        } else {
            match qpack_decoder
                .decode_repr(reader.slice(payload_len)?, id)
                .map_err(DecodeError::QpackError)?
            {
                FieldDecodeState::Blocked => {
                    frames.push(FrameKind::Blocked);
                    self.subtract_payload_len(payload_len)?;
                    self.offset += payload_len;
                    Ok(DecodePartRes::ReturnOuter)
                }
                FieldDecodeState::Decoded => {
                    self.offset += payload_len;
                    self.get_qpack_decoded_header(frames, qpack_decoder, id, reader.cap())
                }
            }
        }
    }

    fn decode_variable_payload(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            Ok(id) => {
                match self.frame_type {
                    Some(frame::GOAWAY_FRAME_TYPE) => {
                        let frame =
                            Frame::new(frame::GOAWAY_FRAME_TYPE, Payload::Goaway(GoAway::new(id)));
                        frames.push(FrameKind::Complete(Box::from(frame)));
                    }
                    Some(frame::MAX_PUSH_ID_FRAME_TYPE) => {
                        let frame = Frame::new(
                            frame::MAX_PUSH_ID_FRAME_TYPE,
                            Payload::MaxPushId(MaxPushId::new(id)),
                        );
                        frames.push(FrameKind::Complete(Box::from(frame)));
                    }
                    Some(frame::CANCEL_PUSH_FRAME_TYPE) => {
                        let frame = Frame::new(
                            frame::CANCEL_PUSH_FRAME_TYPE,
                            Payload::CancelPush(CancelPush::new(id)),
                        );
                        frames.push(FrameKind::Complete(Box::from(frame)));
                    }
                    _ => return Err(DecodeError::UnexpectedFrame(self.frame_type.unwrap()).into()),
                }
                self.offset += reader.index();
                let remaining = reader.cap();
                self.clear_frame();
                self.set_decode_state(DecodeState::FrameType);
                if remaining == 0 {
                    self.clear_buffer();
                    Ok(DecodePartRes::ReturnOuter)
                } else {
                    Ok(DecodePartRes::Continue)
                }
            }
            Err(_) => {
                frames.push(FrameKind::Partial);
                Ok(DecodePartRes::ReturnOuter)
            }
        }
    }

    fn decode_settings_payload(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        let payload_len = self.remain_payload_len();
        if (reader.cap() as u64) < payload_len {
            frames.push(FrameKind::Partial);
            return Ok(DecodePartRes::ReturnOuter);
        }
        let mut settings = Settings::default();

        let mut addition = Vec::new();

        while (reader.index() as u64) < payload_len {
            let key = match reader.get_varint() {
                Ok(id) => id,
                Err(_) => return Err(FrameSizeError(payload_len).into()),
            };
            let value = match reader.get_varint() {
                Ok(val) => val,
                Err(_) => return Err(FrameSizeError(payload_len).into()),
            };

            match key {
                frame::SETTING_QPACK_MAX_TABLE_CAPACITY => {
                    settings.set_qpack_max_table_capacity(value);
                }
                frame::SETTING_ENABLE_CONNECT_PROTOCOL => {
                    settings.set_connect_protocol_enabled(value)
                }
                frame::SETTING_H3_DATAGRAM => settings.set_h3_datagram(value),
                frame::SETTING_MAX_FIELD_SECTION_SIZE => settings.set_max_field_section_size(value),
                frame::SETTING_QPACK_BLOCKED_STREAMS => settings.set_qpack_block_stream(value),
                0x0 | 0x2 | 0x3 | 0x4 | 0x5 => return Err(UnsupportedSetting(key).into()),
                _ => addition.push((key, value)),
            }
        }

        if !addition.is_empty() {
            settings.set_additional(addition);
        }
        let frame = Frame::new(SETTINGS_FRAME_TYPE, Payload::Settings(settings));
        frames.push(FrameKind::Complete(Box::from(frame)));
        let remaining = reader.cap();
        self.clear_frame();
        self.set_decode_state(DecodeState::FrameType);
        if remaining == 0 {
            self.clear_buffer();
            Ok(DecodePartRes::ReturnOuter)
        } else {
            Ok(DecodePartRes::Continue)
        }
    }

    fn decode_push_payload(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        match reader.get_varint() {
            Ok(id) => {
                self.push_frame_id = Some(id);
                self.subtract_payload_len(reader.index())?;
                self.set_decode_state(DecodeState::HeadersPayload);
                Ok(DecodePartRes::Continue)
            }
            Err(_) => {
                frames.push(FrameKind::Partial);
                Ok(DecodePartRes::ReturnOuter)
            }
        }
    }

    fn decode_unknown_payload(&mut self, frames: &mut Frames) -> Result<DecodePartRes, H3Error> {
        let mut reader = ReadableBytes::from(&self.buffer.as_slice()[self.offset..]);
        // Note: None is impossible for `stream.payload_len`.
        let payload_len = self.remain_payload_len() as usize;
        if reader.cap() < payload_len {
            let remaining = reader.cap();
            self.clear_buffer();
            self.subtract_payload_len(remaining)?;
            frames.push(FrameKind::Partial);
            Ok(DecodePartRes::ReturnOuter)
        } else {
            reader.slice(payload_len)?;
            // Reader will renew by stream.offset, so don't need to reset.
            self.offset += payload_len;
            let remaining = reader.cap();
            self.clear_frame();
            self.set_decode_state(DecodeState::FrameType);
            if remaining == 0 {
                self.clear_buffer();
                Ok(DecodePartRes::ReturnOuter)
            } else {
                Ok(DecodePartRes::Continue)
            }
        }
    }
}

fn decode_type(integer: u64) -> StreamType {
    match integer {
        0x0 => StreamType::Control,
        0x1 => StreamType::Push,
        0x2 => StreamType::QpackEncoder,
        0x3 => StreamType::QpackDecoder,
        _ => StreamType::Unknown,
    }
}

#[cfg(test)]
mod h3_decoder {
    use crate::h3::{FrameDecoder, FrameKind, Payload, StreamMessage};

    /// UT test cases for `FrameDecoder` decoding `Settings` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a buf with encoded Settings frame bytes.
    /// 3. Decode the bytes with unidirectional stream id.
    /// 4. Checkout if the stream type is correct.
    /// 5. Checkout if the frame type is correct.
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn ut_decode_stream_control_with_setting() {
        let mut decoder = FrameDecoder::new(100, 500);
        let buf: [u8; 24] = [
            0x0, 0x4, 0x15, 0x6, 0x80, 0x1, 0x0, 0x0, 0xF5, 0xEF, 0x9, 0x12, 0x8C, 0xC, 0x8E, 0x72,
            0xDD, 0xB5, 0xB5, 0x15, 0xCD, 0x85, 0x44, 0xF8,
        ];
        let res = decoder.decode(3, &buf).unwrap();
        if let StreamMessage::Control(frames) = res {
            assert_eq!(frames.len(), 1);
            for frame_kind in frames.iter() {
                if let FrameKind::Complete(frame) = frame_kind {
                    assert_eq!(*frame.frame_type(), 0x4);
                    if let Payload::Settings(_setting) = frame.payload() {
                        return;
                    }
                }
            }
        }
        assert!(false)
    }

    /// UT test cases for `FrameDecoder` decoding `Headers` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a buf with encoded Headers frame bytes.
    /// 3. Decode the bytes with bidirectional stream id.
    /// 4. Checkout if the stream type is correct.
    /// 5. Checkout if the frame type is correct.
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn ut_decode_stream_request_with_header() {
        let mut decoder = FrameDecoder::new(100, 500);
        let buf: [u8; 117] = [
            0x1, 0x40, 0x72, 0x0, 0x0, 0xD9, 0x56, 0x96, 0xC3, 0x61, 0xBE, 0x94, 0xB, 0xCA, 0x6A,
            0x22, 0x54, 0x10, 0x4, 0xD2, 0x80, 0x15, 0xC6, 0x99, 0xB8, 0x27, 0x54, 0xC5, 0xA3,
            0x7F, 0x5F, 0x1D, 0x87, 0x49, 0x7C, 0xA5, 0x89, 0xD3, 0x4D, 0x1F, 0x54, 0x85, 0x8,
            0x9B, 0x7D, 0xB7, 0xFF, 0x5F, 0x4D, 0x87, 0x25, 0x7, 0xB6, 0x49, 0x68, 0x1D, 0x85,
            0x2D, 0x24, 0xAB, 0x58, 0x3F, 0x5F, 0x8F, 0x7A, 0x46, 0x9B, 0x11, 0x5B, 0x64, 0x92,
            0x46, 0xF1, 0x23, 0x7C, 0x8B, 0x67, 0x87, 0xF3, 0x5F, 0x44, 0x90, 0x9D, 0x98, 0x3F,
            0x9B, 0x8D, 0x34, 0xCF, 0xF3, 0xF6, 0xA5, 0x23, 0x81, 0xE7, 0x1A, 0x0, 0x3F, 0x2F, 0x3,
            0x41, 0x6C, 0xEE, 0x5B, 0x16, 0x49, 0xA9, 0x35, 0x53, 0x7F, 0x86, 0x24, 0xB8, 0x3C,
            0xA7, 0x5D, 0x86,
        ];
        let res = decoder.decode(0, &buf).unwrap();
        if let StreamMessage::Request(frames) = res {
            assert_eq!(frames.len(), 1);
            for frame_kind in frames.iter() {
                if let FrameKind::Complete(frame) = frame_kind {
                    assert_eq!(*frame.frame_type(), 0x1);
                    if let Payload::Headers(header) = frame.payload() {
                        assert!(header.get_instruction().is_none());
                        let part = header.get_part();
                        let (pseudo, _headers) = part.parts();
                        assert_eq!(pseudo.status(), Some("200"));
                        return;
                    }
                }
            }
        }
        assert!(false)
    }

    /// UT test cases for `FrameDecoder` decoding `Unknown` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a buf with encoded unknown type frame bytes.
    /// 3. Decode the bytes with bidirectional stream id.
    /// 4. Checkout if the stream type is correct.
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn ut_decode_stream_request_with_unknown() {
        let mut decoder = FrameDecoder::new(100, 500);
        let buf: [u8; 9] = [0xDF, 0x6B, 0xED, 0xB9, 0x11, 0x75, 0x93, 0x91, 0x0];
        let res = decoder.decode(0, &buf).unwrap();
        if let StreamMessage::Request(frames) = res {
            assert_eq!(frames.len(), 0);
            return;
        }
        assert!(false)
    }
}
