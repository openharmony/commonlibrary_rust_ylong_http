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

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use ylong_runtime::iter::parallel::ParSplit;

use crate::h3::error::CommonError::{BufferTooShort, InternalError};
use crate::h3::error::DecodeError::UnexpectedFrame;
use crate::h3::error::EncodeError::{
    NoCurrentFrame, RepeatSetFrame, UnknownFrameType, WrongTypeFrame,
};
use crate::h3::error::{DecodeError, EncodeError, H3Error};
use crate::h3::frame::{Headers, Payload};
use crate::h3::octets::{ReadableBytes, WritableBytes};
use crate::h3::qpack::encoder::EncodeMessage;
use crate::h3::qpack::error::{ErrorCode, QpackError};
use crate::h3::qpack::table::DynamicTable;
use crate::h3::qpack::{DecoderInst, QpackEncoder};
use crate::h3::EncodeError::TooManySettings;
use crate::h3::{frame, is_bidirectional, octets, Frame};

#[derive(PartialEq, Debug)]
enum FrameEncoderState {
    // The initial state for the frame encoder.
    Idle,
    FrameComplete,
    // Header Frame
    EncodingHeadersFrame,
    EncodingHeadersPayload,
    // Data Frame
    EncodingDataFrame,
    EncodingDataPayload,
    // CancelPush Frame
    EncodingCancelPushFrame,
    // Settings Frame
    EncodingSettingsFrame,
    EncodingSettingsPayload,
    // Goaway Frame
    EncodingGoawayFrame,
    // MaxPushId Frame
    EncodingMaxPushIdFrame,
}

struct EncodedH3Stream {
    stream_id: u64,
    headers_message: Option<EncHeaders>,
    current_frame: Option<Frame>,
    state: FrameEncoderState,
    payload_offset: usize,
}

pub(crate) struct EncHeaders {
    message: EncodeMessage,
    repr_offset: usize,
    inst_offset: usize,
}

/// HTTP3 frame encoder, which serializes a Frame into a byte stream in the
/// http3 protocol.
///
/// # Examples
///
/// ```
/// use ylong_http::h3::{Data, Frame, FrameEncoder, Payload};
///
/// let mut encoder = FrameEncoder::default();
/// let data_frame = Frame::new(
///     0,
///     Payload::Data(Data::new(vec![b'h', b'e', b'l', b'l', b'o'])),
/// );
/// encoder.set_frame(0, data_frame).unwrap();
/// let mut res = [0u8; 1024];
/// let mut ins = [0u8; 1024];
/// let message = encoder.encode(0, &mut res, &mut ins).unwrap();
/// ```
#[derive(Default)]
pub struct FrameEncoder {
    qpack_encoder: QpackEncoder,
    streams: HashMap<u64, EncodedH3Stream>,
}

pub struct EncodedSize {
    frame_size: usize,
    inst_size: usize,
}

impl FrameEncoder {
    /// Sets the maximum dynamic table capacity,
    /// which must not exceed the SETTINGS_QPACK_MAX_TABLE_CAPACITY sent by the
    /// peer Decoder.
    pub fn set_max_table_capacity(&mut self, max_cap: usize) -> Result<(), H3Error> {
        self.qpack_encoder
            .set_max_table_capacity(max_cap)
            .map_err(|e| EncodeError::QpackError(e).into())
    }

    /// Sets the SETTINGS_QPACK_BLOCKED_STREAMS sent by the peer Decoder.
    pub fn set_max_blocked_stream_size(&mut self, max_blocked: usize) {
        self.qpack_encoder.set_max_blocked_stream_size(max_blocked)
    }

    /// Sets the current frame to be encoded by the `FrameEncoder`. The state of
    /// the encoder is updated based on the payload type of the frame.
    pub fn set_frame(&mut self, stream_id: u64, frame: Frame) -> Result<(), H3Error> {
        let stream = self
            .streams
            .entry(stream_id)
            .or_insert(EncodedH3Stream::new(stream_id));

        match stream.state {
            FrameEncoderState::Idle | FrameEncoderState::FrameComplete => {}
            _ => return Err(RepeatSetFrame.into()),
        }
        stream.current_frame = Some(frame);
        // set frame state
        if let Some(ref frame) = stream.current_frame {
            match *frame.frame_type() {
                frame::HEADERS_FRAME_TYPE => {
                    if let Payload::Headers(h) = frame.payload() {
                        self.qpack_encoder.set_parts(h.get_part());
                        stream.state = FrameEncoderState::EncodingHeadersFrame;
                    }
                }
                frame::DATA_FRAME_TYPE => stream.state = FrameEncoderState::EncodingDataFrame,
                frame::CANCEL_PUSH_FRAME_TYPE => {
                    stream.state = FrameEncoderState::EncodingCancelPushFrame
                }
                frame::SETTINGS_FRAME_TYPE => {
                    stream.state = FrameEncoderState::EncodingSettingsFrame
                }
                frame::GOAWAY_FRAME_TYPE => stream.state = FrameEncoderState::EncodingGoawayFrame,
                frame::MAX_PUSH_ID_FRAME_TYPE => {
                    stream.state = FrameEncoderState::EncodingMaxPushIdFrame
                }
                _ => {
                    return Err(UnknownFrameType.into());
                }
            };
        }
        Ok(())
    }

    /// Decode the instructions sent by the peer decoder stream.
    pub fn decode_remote_inst(&mut self, buf: &[u8]) -> Result<(), H3Error> {
        self.qpack_encoder
            .decode_ins(buf)
            .map_err(|e| H3Error::Decode(DecodeError::QpackError(e)))
    }

    /// Serializes a Frame into a byte stream in the http3 protocol.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h3::{Data, Frame, FrameEncoder, Payload};
    ///
    /// let mut encoder = FrameEncoder::default();
    /// let data_frame = Frame::new(
    ///     0,
    ///     Payload::Data(Data::new(vec![b'h', b'e', b'l', b'l', b'o'])),
    /// );
    /// encoder.set_frame(0, data_frame).unwrap();
    /// let mut res = [0u8; 1024];
    /// let mut ins = [0u8; 1024];
    /// let message = encoder.encode(0, &mut res, &mut ins).unwrap();
    /// ```
    pub fn encode(
        &mut self,
        stream_id: u64,
        frame_buf: &mut [u8],
        inst_buf: &mut [u8],
    ) -> Result<(usize, usize), H3Error> {
        if frame_buf.len() < 1024 {
            return Err(BufferTooShort.into());
        }
        let (mut frame_bytes, inst_bytes) = (0, 0);

        let stream = self.streams.get_mut(&stream_id).ok_or(InternalError)?;
        while frame_bytes < frame_buf.len() {
            match stream.state {
                FrameEncoderState::Idle | FrameEncoderState::FrameComplete => {
                    break;
                }
                FrameEncoderState::EncodingHeadersFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    stream.state = FrameEncoderState::EncodingHeadersPayload;
                }
                FrameEncoderState::EncodingHeadersPayload => {
                    let (payload, inst) = stream.encode_headers_payload(
                        &mut self.qpack_encoder,
                        &mut frame_buf[frame_bytes..],
                        inst_buf,
                    )?;
                    return Ok((payload + frame_bytes, inst));
                }

                FrameEncoderState::EncodingDataFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    let len = stream.encode_data_len(&mut frame_buf[frame_bytes..])?;
                    frame_bytes += len;
                    stream.state = FrameEncoderState::EncodingDataPayload;
                }
                FrameEncoderState::EncodingDataPayload => {
                    return stream
                        .encode_data_payload(&mut frame_buf[frame_bytes..])
                        .map(|size| (size + frame_bytes, 0));
                }

                FrameEncoderState::EncodingCancelPushFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    return stream
                        .encode_cancel_push(&mut frame_buf[frame_bytes..])
                        .map(|size| (size + frame_bytes, 0));
                }

                FrameEncoderState::EncodingSettingsFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    let len = stream.encode_settings_len(&mut frame_buf[frame_bytes..])?;
                    frame_bytes += len;
                    stream.state = FrameEncoderState::EncodingSettingsPayload;
                }
                FrameEncoderState::EncodingSettingsPayload => {
                    return stream
                        .encode_settings_payload(&mut frame_buf[frame_bytes..])
                        .map(|size| (size + frame_bytes, 0));
                }

                FrameEncoderState::EncodingGoawayFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    return stream
                        .encode_goaway(&mut frame_buf[frame_bytes..])
                        .map(|size| (size + frame_bytes, 0));
                }

                FrameEncoderState::EncodingMaxPushIdFrame => {
                    let frame_type = stream.encode_frame_type(frame_buf)?;
                    frame_bytes += frame_type;
                    return stream
                        .encode_max_push_id(&mut frame_buf[frame_bytes..])
                        .map(|size| (size + frame_bytes, 0));
                }
            }
        }
        Ok((frame_bytes, inst_bytes))
    }

    /// Cleans the stream information when the stream normally ends.
    pub fn finish_stream(&mut self, id: u64) -> Result<(), H3Error> {
        if is_bidirectional(id) {
            self.qpack_encoder
                .finish_stream(id)
                .map_err(|e| H3Error::Encode(e.into()))?;
        }
        self.streams.remove(&id);
        Ok(())
    }
}

impl EncHeaders {
    pub(crate) fn new(message: EncodeMessage) -> Self {
        Self {
            message,
            repr_offset: 0,
            inst_offset: 0,
        }
    }
    pub(crate) fn message(&self) -> &EncodeMessage {
        &self.message
    }

    pub(crate) fn repr_offset(&self) -> usize {
        self.repr_offset
    }

    pub(crate) fn inst_offset(&self) -> usize {
        self.inst_offset
    }

    pub(crate) fn repr_offset_inc(&mut self, increment: usize) {
        self.repr_offset += increment
    }

    pub(crate) fn inst_offset_inc(&mut self, increment: usize) {
        self.inst_offset += increment
    }

    pub(crate) fn remaining_repr(&self) -> usize {
        self.message.fields().len() - self.repr_offset
    }

    pub(crate) fn remaining_inst(&self) -> usize {
        self.message.inst().len() - self.inst_offset
    }
}

impl EncodedSize {
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    pub fn inst_size(&self) -> usize {
        self.inst_size
    }

    pub fn new(frame_size: usize, inst_size: usize) -> Self {
        Self {
            frame_size,
            inst_size,
        }
    }
}

impl EncodedH3Stream {
    pub(crate) fn new(stream_id: u64) -> Self {
        Self {
            stream_id,
            headers_message: None,
            current_frame: None,
            state: FrameEncoderState::Idle,
            payload_offset: 0,
        }
    }

    fn encode_settings_payload(&mut self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        let mut written = 0;
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Settings(settings) = frame.payload() {
                // Ensure that it can be completed in a one go.
                self.state = FrameEncoderState::FrameComplete;

                if let Some(v) = settings.max_fied_section_size() {
                    written += encode_var_integer(
                        frame::SETTING_MAX_FIELD_SECTION_SIZE,
                        &mut frame_buf[written..],
                    )?;
                    written += encode_var_integer(v, &mut frame_buf[written..])?;
                }
                if let Some(v) = settings.connect_protocol_enabled() {
                    written += encode_var_integer(
                        frame::SETTING_ENABLE_CONNECT_PROTOCOL,
                        &mut frame_buf[written..],
                    )?;
                    written += encode_var_integer(v, &mut frame_buf[written..])?;
                }
                if let Some(v) = settings.qpack_max_table_capacity() {
                    written += encode_var_integer(
                        frame::SETTING_QPACK_MAX_TABLE_CAPACITY,
                        &mut frame_buf[written..],
                    )?;
                    written += encode_var_integer(v, &mut frame_buf[written..])?;
                }
                if let Some(v) = settings.qpack_block_stream() {
                    written += encode_var_integer(
                        frame::SETTING_QPACK_BLOCKED_STREAMS,
                        &mut frame_buf[written..],
                    )?;
                    written += encode_var_integer(v, &mut frame_buf[written..])?;
                }
                if let Some(v) = settings.h3_datagram() {
                    written +=
                        encode_var_integer(frame::SETTING_H3_DATAGRAM, &mut frame_buf[written..])?;
                    written += encode_var_integer(v, &mut frame_buf[written..])?;
                }
                if let Some(v) = settings.additional() {
                    for (key, value) in v.iter() {
                        written += encode_var_integer(*key, &mut frame_buf[written..])?;
                        written += encode_var_integer(*value, &mut frame_buf[written..])?;
                    }
                }
                Ok(written)
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_headers_repr_and_inst(
        &mut self,
        frame_buf: &mut [u8],
        inst_buf: &mut [u8],
    ) -> Result<(usize, usize), H3Error> {
        let repr_writen = self.encode_headers_repr(frame_buf);
        let inst_writen = self.encode_qpack_inst(inst_buf);
        if let Some(ref message) = self.headers_message {
            if message.remaining_repr() == 0 && message.remaining_inst() == 0 {
                self.state = FrameEncoderState::FrameComplete;
                self.headers_message = None;
            }
        }
        Ok((repr_writen, inst_writen))
    }

    fn encode_headers_with_qpack(
        &mut self,
        qpack_encoder: &mut QpackEncoder,
        frame_buf: &mut [u8],
        inst_buf: &mut [u8],
    ) -> Result<(usize, usize), H3Error> {
        if self.headers_message.is_none() {
            let message = qpack_encoder.encode(self.stream_id);
            let payload_size = message.fields().len();
            self.headers_message = Some(EncHeaders::new(message));
            // encode headers frame payload length
            let encoded_payload_size = encode_var_integer(payload_size as u64, frame_buf)?;
            let (frame_size, inst_size) = self
                .encode_headers_repr_and_inst(&mut frame_buf[encoded_payload_size..], inst_buf)?;
            Ok((frame_size + encoded_payload_size, inst_size))
        } else {
            self.encode_headers_repr_and_inst(frame_buf, inst_buf)
        }
    }

    fn encode_headers_repr(&mut self, frame_buf: &mut [u8]) -> usize {
        if let Some(mut enc_headers) = self.headers_message.take() {
            let mut written = 0;
            let repr_size = enc_headers.remaining_repr();
            let cap = frame_buf.len();
            if cap >= repr_size {
                frame_buf[..repr_size].copy_from_slice(
                    &enc_headers.message().fields().as_slice()[enc_headers.repr_offset()..],
                );
                written += repr_size;
                // finish encode headers
                enc_headers.repr_offset_inc(repr_size);
            } else {
                frame_buf.copy_from_slice(
                    &enc_headers.message().fields().as_slice()
                        [enc_headers.repr_offset()..enc_headers.repr_offset() + cap],
                );
                written += cap;
                enc_headers.repr_offset_inc(cap);
            }
            self.headers_message = Some(enc_headers);
            return written;
        }
        0
    }

    fn encode_qpack_inst(&mut self, inst_buf: &mut [u8]) -> usize {
        if let Some(mut enc_headers) = self.headers_message.take() {
            let mut written = 0;
            let inst_size = enc_headers.remaining_inst();
            let cap = inst_buf.len();
            if cap >= inst_size {
                inst_buf[..inst_size].copy_from_slice(
                    &enc_headers.message().inst().as_slice()[enc_headers.inst_offset()..],
                );
                written += inst_size;
                // finish encode headers
                enc_headers.inst_offset_inc(inst_size);
            } else {
                inst_buf.copy_from_slice(
                    &enc_headers.message().inst().as_slice()
                        [enc_headers.inst_offset()..enc_headers.inst_offset() + cap],
                );
                written += cap;
                enc_headers.inst_offset_inc(cap);
            }
            self.headers_message = Some(enc_headers);
            return written;
        }
        0
    }

    fn encode_frame_type(&self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            encode_var_integer(*frame.frame_type(), frame_buf)
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_data_len(&self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Data(d) = frame.payload() {
                encode_var_integer((*d).data().len() as u64, frame_buf)
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_cancel_push(&mut self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        let frame = self
            .current_frame
            .as_ref()
            .ok_or(H3Error::Encode(NoCurrentFrame))?;
        if let Payload::CancelPush(push) = frame.payload() {
            let size =
                encode_var_integer(octets::varint_len(*push.get_push_id()) as u64, frame_buf)?;
            // Ensure that it can be completed in a one go.
            self.state = FrameEncoderState::FrameComplete;
            encode_var_integer(*push.get_push_id(), &mut frame_buf[size..])
        } else {
            Err(WrongTypeFrame.into())
        }
    }

    fn encode_goaway(&mut self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Goaway(away) = frame.payload() {
                let size =
                    encode_var_integer(octets::varint_len(*away.get_id()) as u64, frame_buf)?;
                // Ensure that it can be completed in a one go.
                self.state = FrameEncoderState::FrameComplete;

                encode_var_integer(*away.get_id(), &mut frame_buf[size..])
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_max_push_id(&mut self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::MaxPushId(push) = frame.payload() {
                let size =
                    encode_var_integer(octets::varint_len(*push.get_id()) as u64, frame_buf)?;
                // Ensure that it can be completed in a one go.
                self.state = FrameEncoderState::FrameComplete;

                encode_var_integer(*push.get_id(), &mut frame_buf[size..])
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_settings_len(&self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        let mut written = 0;
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Settings(settings) = frame.payload() {
                if let Some(v) = settings.max_fied_section_size() {
                    written += octets::varint_len(frame::SETTING_MAX_FIELD_SECTION_SIZE);
                    written += octets::varint_len(v);
                }
                if let Some(v) = settings.connect_protocol_enabled() {
                    written += octets::varint_len(frame::SETTING_ENABLE_CONNECT_PROTOCOL);
                    written += octets::varint_len(v);
                }
                if let Some(v) = settings.qpack_max_table_capacity() {
                    written += octets::varint_len(frame::SETTING_QPACK_MAX_TABLE_CAPACITY);
                    written += octets::varint_len(v);
                }
                if let Some(v) = settings.qpack_block_stream() {
                    written += octets::varint_len(frame::SETTING_QPACK_BLOCKED_STREAMS);
                    written += octets::varint_len(v);
                }
                if let Some(v) = settings.h3_datagram() {
                    written += octets::varint_len(frame::SETTING_H3_DATAGRAM);
                    written += octets::varint_len(v);
                }
                if let Some(v) = settings.additional() {
                    // ensure frame buf is enough capacity.
                    if v.len() > 50 {
                        return Err(TooManySettings.into());
                    }
                    for (key, value) in v.iter() {
                        written += octets::varint_len(*key);
                        written += octets::varint_len(*value);
                    }
                }
                let var_written = encode_var_integer(written as u64, frame_buf)?;
                Ok(var_written)
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_headers_payload(
        &mut self,
        qpack_encoder: &mut QpackEncoder,
        frame_buf: &mut [u8],
        inst_buf: &mut [u8],
    ) -> Result<(usize, usize), H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Headers(_headers) = frame.payload() {
                self.encode_headers_with_qpack(qpack_encoder, frame_buf, inst_buf)
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }

    fn encode_data_payload(&mut self, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
        if let Some(frame) = self.current_frame.as_ref() {
            if let Payload::Data(d) = frame.payload() {
                let data = d.data();
                let buf_size = frame_buf.len();
                let remaining = data.len() - self.payload_offset;
                if buf_size >= remaining {
                    frame_buf[..remaining].copy_from_slice(&data.as_slice()[self.payload_offset..]);
                    self.payload_offset = 0;
                    self.state = FrameEncoderState::FrameComplete;
                    Ok(remaining)
                } else {
                    frame_buf.copy_from_slice(
                        &data.as_slice()[self.payload_offset..self.payload_offset + buf_size],
                    );
                    self.payload_offset += buf_size;
                    Ok(buf_size)
                }
            } else {
                Err(WrongTypeFrame.into())
            }
        } else {
            Err(NoCurrentFrame.into())
        }
    }
}

fn encode_var_integer(src: u64, frame_buf: &mut [u8]) -> Result<usize, H3Error> {
    let mut writable_buf = WritableBytes::from(frame_buf);
    writable_buf.write_varint(src)?;
    let size = writable_buf.index();
    Ok(size)
}

#[cfg(test)]
mod h3_encoder {
    use crate::h3::qpack::table::NameField;
    use crate::h3::{Data, Frame, FrameEncoder, Headers, Parts, Payload, PseudoHeaders, Settings};

    /// UT test cases for `FrameEncoder` encoding `Settings` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Settings`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame with request stream id.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encoder_request_stream_settings() {
        let mut encoder = FrameEncoder::default();
        let setting = Settings::default();
        let setting = Frame::new(0x4, Payload::Settings(setting));
        encoder.set_frame(1, setting).unwrap();
        let mut data_buf = [0u8; 1024];
        let mut inst_buf = [0u8; 1024];
        let res = encoder.encode(0, &mut data_buf, &mut inst_buf);
        assert!(res.is_err());
    }

    /// UT test cases for `FrameEncoder` encoding `Settings` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Settings`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame with control stream id.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encoder_control_stream_settings() {
        let mut encoder = FrameEncoder::default();
        let mut setting = Settings::default();
        setting.set_max_field_section_size(1024);
        setting.set_qpack_block_stream(50);
        setting.set_qpack_max_table_capacity(1024);
        let setting = Frame::new(0x4, Payload::Settings(setting));
        encoder.set_frame(1, setting).unwrap();
        let mut data_buf = [0u8; 1024];
        let mut inst_buf = [0u8; 1024];

        let (data_idx, inst_idx) = encoder.encode(1, &mut data_buf, &mut inst_buf).unwrap();
        assert_eq!([4, 8, 6, 68, 0, 1, 68, 0, 7, 50], data_buf[..data_idx]);
        assert_eq!(inst_idx, 0);
    }

    /// UT test cases for `FrameEncoder` encoding `Headers` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Headers`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame with request stream id.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encoder_request_stream_header() {
        let mut encoder = FrameEncoder::default();
        encoder.set_max_table_capacity(400).unwrap();
        let mut parts = Parts::new();
        parts.update(
            NameField::Other("test-header".to_string()),
            "test-header".to_string(),
        );
        parts.update(NameField::Method, "GET".to_string());
        parts.update(NameField::Scheme, "HTTPS".to_string());
        parts.update(NameField::Authority, "www.example.com".to_string());
        let headers = Headers::new(parts);
        let header = Frame::new(1, Payload::Headers(headers));
        encoder.set_frame(0, header).unwrap();
        let mut data_buf = [0u8; 1024];
        let mut inst_buf = [0u8; 1024];

        let (data_idx, inst_idx) = encoder.encode(0, &mut data_buf, &mut inst_buf).unwrap();
        assert_eq!(
            data_buf[..data_idx],
            [
                1, 43, 0, 0, 209, 95, 7, 133, 199, 191, 126, 189, 223, 80, 140, 241, 227, 194, 229,
                242, 58, 107, 160, 171, 144, 244, 255, 43, 73, 80, 149, 167, 40, 228, 45, 159, 136,
                73, 80, 149, 167, 40, 228, 45, 159
            ]
        );
        assert_eq!(&inst_buf[..inst_idx], [63, 241, 2]);
    }

    /// UT test cases for `FrameEncoder` encoding `Data` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Data`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame with request stream id.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encoder_request_stream_data() {
        let mut encoder = FrameEncoder::default();
        let data_body = Data::new(Vec::from("hello"));
        let data = Frame::new(0, Payload::Data(data_body));

        encoder.set_frame(0, data).unwrap();
        let mut data_buf = [0u8; 1024];
        let mut inst_buf = [0u8; 1024];
        let (data_idx, inst_idx) = encoder.encode(0, &mut data_buf, &mut inst_buf).unwrap();
        assert_eq!([0, 5, 104, 101, 108, 108, 111], data_buf[..data_idx]);
        assert_eq!(inst_idx, 0)
    }
}
