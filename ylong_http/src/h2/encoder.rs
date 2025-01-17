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

use crate::h2::frame::{FrameFlags, FrameType, Payload, Priority, Setting};
use crate::h2::{Frame, Goaway, HpackEncoder, Settings};

// TODO: Classify encoder errors per RFC specifications into categories like
// stream or connection errors. Identify specific error types such as
// Frame_size_error/Protocol Error.
const DEFAULT_MAX_FRAME_SIZE: usize = 16384;

const DEFAULT_HEADER_TABLE_SIZE: usize = 4096;

#[derive(Debug)]
pub enum FrameEncoderErr {
    EncodingData,
    UnexpectedPayloadType,
    NoCurrentFrame,
    InternalError,
    HeadersNotEnd,
}

#[derive(PartialEq, Debug)]
enum FrameEncoderState {
    // The initial state for the frame encoder.
    Idle,
    // The initial state for encoding the HEADERS frame, including the frame header and the Field
    // Block Fragment.
    EncodingHeadersFrame,
    // The state for encoding the payload of the HEADERS frame, including the header block
    // fragment.
    EncodingHeadersPayload,
    // The state for encoding the padding octets for the HEADERS frame, if the PADDED flag is set.
    EncodingHeadersPadding,
    // TODO compare to max_header_list_size
    // The state for encoding CONTINUATION frames if the header block exceeds the max_frame_size.
    EncodingContinuationFrames,
    // The final state, indicating that the HEADERS frame and any necessary CONTINUATION frames
    // have been fully encoded.
    HeadersComplete,
    // The initial state for encoding the DATA frame, including the frame header and the Pad
    // Length field (if PADDED flag is set).
    EncodingDataHeader,
    // The state for encoding the actual data payload of the DATA frame.
    EncodingDataPayload,
    // The state for encoding the padding octets for the DATA frame, if the PADDED flag is set.
    EncodingDataPadding,
    // The initial state for encoding the SETTINGS frame, including the frame header.
    EncodingSettingsFrame,
    // The state for encoding the SETTINGS frame payload.
    EncodingSettingsPayload,
    // The initial state for encoding the GOAWAY frame, including the frame header.
    EncodingGoawayFrame,
    // The state for encoding the GOAWAY frame payload.
    EncodingGoawayPayload,
    // The initial state for encoding the WINDOW_UPDATE frame, including the frame header.
    EncodingWindowUpdateFrame,
    // The state for encoding the WINDOW_UPDATE frame payload.
    EncodingWindowUpdatePayload,
    // The initial state for encoding the PRIORITY frame, including the frame header.
    EncodingPriorityFrame,
    // The state for encoding Priority frame payload.
    EncodingPriorityPayload,
    // The initial state for encoding the RST_STREAM frame, including the frame header.
    EncodingRstStreamFrame,
    // The state for encoding the RST_STREAM frame payload.
    EncodingRstStreamPayload,
    // The initial state for encoding the PING frame, including the frame header.
    EncodingPingFrame,
    // The state for encoding the PING frame payload.
    EncodingPingPayload,
    // The final state, indicating that the DATA frame has been fully encoded.
    DataComplete,
}

/// A structure for encoding frames into bytes, supporting the serialization of
/// HTTP/2 Frames. It maintains the state of the current frame being encoded and
/// also handles the fragmentation of frames.
pub struct FrameEncoder {
    current_frame: Option<Frame>,
    // `max_frame_size` is actually useless in the Encoder for headers frame and continuation
    // frame, because the frame length does not exceed the length of the
    // `header_payload_buffer`
    max_frame_size: usize,
    max_header_list_size: usize,
    hpack_encoder: HpackEncoder,
    state: FrameEncoderState,
    encoded_bytes: usize,
    data_offset: usize,
    // `remaining_header_payload` will always be smaller than the minimum max_frame_size,
    // because the `header_payload_buffer` length is the minimum max_frame_size.
    remaining_header_payload: usize,
    remaining_payload_bytes: usize,
    is_end_stream: bool,
    is_end_headers: bool,
    header_payload_buffer: Vec<u8>,
    header_payload_index: usize,
}

impl FrameEncoder {
    /// Constructs a new `FrameEncoder` with specified maximum frame size and
    /// maximum header list size.
    pub fn new(max_frame_size: usize, use_huffman: bool) -> Self {
        FrameEncoder {
            current_frame: None,
            max_frame_size,
            max_header_list_size: usize::MAX,
            hpack_encoder: HpackEncoder::new(DEFAULT_HEADER_TABLE_SIZE, use_huffman),
            state: FrameEncoderState::Idle,
            encoded_bytes: 0,
            data_offset: 0,
            remaining_header_payload: 0,
            remaining_payload_bytes: 0,
            is_end_stream: false,
            is_end_headers: false,
            // Initialized to default max frame size(16384).
            header_payload_buffer: vec![0; DEFAULT_MAX_FRAME_SIZE],
            header_payload_index: 0,
        }
    }

    /// Sets the current frame to be encoded by the `FrameEncoder`. The state of
    /// the encoder is updated based on the payload type of the frame.
    pub fn set_frame(&mut self, frame: Frame) -> Result<(), FrameEncoderErr> {
        self.is_end_stream = frame.flags().is_end_stream();
        self.is_end_headers = frame.flags().is_end_headers();
        self.current_frame = Some(frame);
        // Reset the encoded bytes counter
        self.encoded_bytes = 0;

        // Set the initial state based on the frame payload type
        match &self.current_frame {
            Some(frame) => match frame.payload() {
                Payload::Headers(headers) => {
                    if !self.is_end_headers {
                        return Err(FrameEncoderErr::HeadersNotEnd);
                    }
                    self.hpack_encoder.set_parts(headers.get_parts());
                    self.header_payload_index = 0;
                    // TODO: Handle potential scenario where HPACK encoding may not be able to
                    // complete output in one go.
                    let payload_size = self.hpack_encoder.encode(&mut self.header_payload_buffer);
                    self.remaining_header_payload = payload_size;
                    self.state = FrameEncoderState::EncodingHeadersFrame;
                }
                Payload::Priority(_) => self.state = FrameEncoderState::EncodingPriorityFrame,
                Payload::RstStream(_) => self.state = FrameEncoderState::EncodingRstStreamFrame,
                Payload::Ping(_) => self.state = FrameEncoderState::EncodingPingFrame,
                Payload::Data(data) => {
                    self.state = {
                        self.data_offset = 0;
                        self.remaining_payload_bytes = data.size();
                        FrameEncoderState::EncodingDataHeader
                    }
                }
                Payload::Settings(_) => self.state = FrameEncoderState::EncodingSettingsFrame,
                Payload::Goaway(_) => self.state = FrameEncoderState::EncodingGoawayFrame,
                Payload::WindowUpdate(_) => {
                    self.state = FrameEncoderState::EncodingWindowUpdateFrame
                }
                _ => {}
            },
            None => self.state = FrameEncoderState::Idle,
        }
        Ok(())
    }

    /// Encodes the current frame into the given buffer. The state of the
    /// encoder determines which part of the frame is currently being encoded.
    /// This function returns the number of bytes written to the buffer or an
    /// error if the encoding process fails.
    pub fn encode(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let mut written_bytes = 0;

        while written_bytes < buf.len() {
            match self.state {
                FrameEncoderState::Idle
                | FrameEncoderState::HeadersComplete
                | FrameEncoderState::DataComplete => {
                    break;
                }
                FrameEncoderState::EncodingHeadersFrame => {
                    let bytes = self.encode_headers_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingHeadersFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingHeadersPayload => {
                    let bytes = self.encode_headers_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingHeadersPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingHeadersPadding => {
                    let bytes = self.encode_padding(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingHeadersPadding {
                        break;
                    }
                }
                FrameEncoderState::EncodingContinuationFrames => {
                    let bytes = self.encode_continuation_frames(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingContinuationFrames {
                        break;
                    }
                }
                FrameEncoderState::EncodingDataHeader => {
                    let bytes = self.encode_data_header(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingDataHeader {
                        break;
                    }
                }
                FrameEncoderState::EncodingDataPayload => {
                    let bytes = self.encode_data_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingDataPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingDataPadding => {
                    let bytes = self.encode_padding(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingDataPadding {
                        break;
                    }
                }
                FrameEncoderState::EncodingSettingsFrame => {
                    let bytes = self.encode_settings_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingSettingsFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingGoawayFrame => {
                    let bytes = self.encode_goaway_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingGoawayFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingWindowUpdateFrame => {
                    let bytes = self.encode_window_update_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingWindowUpdateFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingSettingsPayload => {
                    let bytes = self.encode_settings_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingSettingsPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingGoawayPayload => {
                    let bytes = self.encode_goaway_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingGoawayPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingWindowUpdatePayload => {
                    let bytes = self.encode_window_update_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingWindowUpdatePayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingPriorityFrame => {
                    let bytes = self.encode_priority_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingPriorityFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingPriorityPayload => {
                    let bytes = self.encode_priority_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingPriorityPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingRstStreamFrame => {
                    let bytes = self.encode_rst_stream_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingRstStreamFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingRstStreamPayload => {
                    let bytes = self.encode_rst_stream_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingRstStreamPayload {
                        break;
                    }
                }
                FrameEncoderState::EncodingPingFrame => {
                    let bytes = self.encode_ping_frame(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingPingFrame {
                        break;
                    }
                }
                FrameEncoderState::EncodingPingPayload => {
                    let bytes = self.encode_ping_payload(&mut buf[written_bytes..])?;
                    written_bytes += bytes;
                    if self.state == FrameEncoderState::EncodingPingPayload {
                        break;
                    }
                }
            }
        }

        Ok(written_bytes)
    }

    /// Updates the provided setting for the current frame if it is a `Settings`
    /// frame.
    pub fn update_setting(&mut self, setting: Setting) {
        if let Some(frame) = &mut self.current_frame {
            if let Payload::Settings(settings) = frame.payload_mut() {
                settings.update_setting(setting);
            }
        }
    }

    /// Sets the maximum frame size for the current encoder instance.
    pub fn update_max_frame_size(&mut self, size: usize) {
        self.max_frame_size = size;
    }

    /// Sets the maximum header table size for the current encoder instance.
    // TODO enable update header table size.
    pub fn update_header_table_size(&mut self, size: usize) {
        self.hpack_encoder.update_max_dynamic_table_size(size)
    }

    // TODO enable update max header list size.
    pub(crate) fn update_max_header_list_size(&mut self, size: usize) {
        self.max_header_list_size = size;
    }

    fn finish_current_frame(&mut self) {
        self.header_payload_index = 0;
        self.is_end_stream = false;
        self.current_frame = None;
        self.is_end_headers = false;
        self.remaining_header_payload = 0;
        self.encoded_bytes = 0;
    }

    fn read_rest_payload(&mut self) {
        self.header_payload_index = 0;
        let payload_size = self.hpack_encoder.encode(&mut self.header_payload_buffer);
        self.remaining_header_payload = payload_size;
    }

    fn try_retrieve_current_frame(&self) -> Result<&Frame, FrameEncoderErr> {
        if let Some(frame) = &self.current_frame {
            return Ok(frame);
        }
        Err(FrameEncoderErr::NoCurrentFrame)
    }

    fn encode_headers_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Headers(_) = frame.payload() {
            // HTTP/2 frame header size is 9 bytes.
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());

            self.iterate_headers_header(frame, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            let bytes_written = bytes_to_write;
            let mut payload_bytes_written = 0;

            if self.encoded_bytes >= frame_header_size {
                payload_bytes_written =
                    self.write_payload(&mut buf[bytes_written..], self.remaining_header_payload);
                self.encoded_bytes += payload_bytes_written;
                self.headers_header_status();
            }

            Ok(bytes_written + payload_bytes_written)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn headers_header_status(&mut self) {
        // Headers encoding does not need to consider max_frame_size
        // because header_payload_buffer must be smaller than max_frame_size.
        self.state = if self.header_payload_index < self.remaining_header_payload {
            FrameEncoderState::EncodingHeadersPayload
        } else if self.hpack_encoder.is_finished() {
            // set_frame ensures that headers must be is_end_headers
            self.finish_current_frame();
            FrameEncoderState::HeadersComplete
        } else {
            self.read_rest_payload();
            FrameEncoderState::EncodingContinuationFrames
        }
    }

    fn iterate_headers_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, item) in buf.iter_mut().enumerate().take(len) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header.
                0..=2 => {
                    let payload_len = self.remaining_header_payload;
                    *item = ((payload_len >> (16 - (8 * header_byte_index))) & 0xFF) as u8;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    *item = FrameType::Headers as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    if self.hpack_encoder.is_finished() {
                        *item = frame.flags().bits();
                    } else {
                        // If not finished, it is followed by a Continuation frame.
                        *item = frame.flags().bits() & 0xFB
                    }
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header.
                5..=8 => {
                    let stream_id_byte_index = header_byte_index - 5;
                    *item = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_headers_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Headers(_) = frame.payload() {
            if buf.is_empty() {
                return Ok(0);
            }

            let payload_bytes_written = self.write_payload(buf, self.remaining_header_payload);
            self.encoded_bytes += payload_bytes_written;

            // Updates the state based on the encoding progress
            self.headers_header_status();

            Ok(payload_bytes_written)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_continuation_frames(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;

        if let Payload::Headers(_) = frame.payload() {
            let available_space = buf.len();
            let frame_header_size = 9;
            // TODO allow available_space < 9
            if available_space < frame_header_size {
                return Ok(0);
            }
            // Encodes CONTINUATION frame header.
            // And this value is always the remaining_header_payload.
            let continuation_frame_len = self.remaining_header_payload.min(self.max_frame_size);
            for (buf_index, item) in buf.iter_mut().enumerate().take(3) {
                *item = ((continuation_frame_len >> (16 - (8 * buf_index))) & 0xFF) as u8;
            }
            buf[3] = FrameType::Continuation as u8;
            let mut new_flags = FrameFlags::empty();
            if self.remaining_header_payload <= self.max_frame_size
                && self.hpack_encoder.is_finished()
                && self.is_end_headers
            {
                // Sets the END_HEADER flag on the last CONTINUATION frame.
                new_flags.set_end_headers(true);
            }
            buf[4] = new_flags.bits();

            for buf_index in 0..4 {
                let stream_id_byte_index = buf_index;
                buf[5 + buf_index] = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
            }

            // Encodes CONTINUATION frame payload.
            let payload_bytes_written =
                self.write_payload(&mut buf[frame_header_size..], continuation_frame_len);
            self.encoded_bytes += payload_bytes_written;

            // Updates the state based on the encoding progress.
            self.headers_header_status();

            Ok(frame_header_size + payload_bytes_written)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_data_header(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Data(_) = frame.payload() {
            // HTTP/2 frame header size is 9 bytes.
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());

            self.iterate_data_header(
                frame,
                buf,
                min(self.remaining_payload_bytes, self.max_frame_size),
                bytes_to_write,
            )?;

            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                // data frame header is finished, reset encoded_bytes.
                self.encoded_bytes = 0;
                self.state = FrameEncoderState::EncodingDataPayload;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_data_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        payload_len: usize,
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, item) in buf.iter_mut().enumerate().take(len) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header.
                0..=2 => {
                    *item = ((payload_len >> (16 - (8 * header_byte_index))) & 0xFF) as u8;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    *item = frame.payload().frame_type() as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    if self.remaining_payload_bytes <= self.max_frame_size {
                        *item = frame.flags().bits();
                    } else {
                        // When max_frame_size is exceeded, a data frame cannot send all data.
                        *item = frame.flags().bits() & 0xFE;
                    }
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header.
                5..=8 => {
                    let stream_id_byte_index = header_byte_index - 5;
                    *item = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_data_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Data(data_frame) = frame.payload() {
            let payload = data_frame.data();
            let writen_bytes = self.encode_slice(buf, payload);
            self.data_offset += writen_bytes;
            self.remaining_payload_bytes -= writen_bytes;

            self.state = if self.remaining_payload_bytes == 0 {
                self.data_offset = 0;
                FrameEncoderState::DataComplete
            } else if self.data_offset == self.max_frame_size {
                self.data_offset = 0;
                FrameEncoderState::EncodingDataHeader
            } else {
                FrameEncoderState::EncodingDataPayload
            };

            Ok(writen_bytes)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_padding(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;

        if frame.flags().is_padded() {
            let padding_len = if let Payload::Data(data_frame) = frame.payload() {
                data_frame.data().len()
            } else {
                return Err(FrameEncoderErr::UnexpectedPayloadType);
            };

            let remaining_padding_bytes = padding_len - self.encoded_bytes;
            let bytes_to_write = remaining_padding_bytes.min(buf.len());

            for item in buf.iter_mut().take(bytes_to_write) {
                // Padding bytes are filled with 0.
                *item = 0;
            }

            self.encoded_bytes += bytes_to_write;

            if self.encoded_bytes == padding_len {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Ok(0) // No padding to encode, so return 0 bytes written.
        }
    }

    fn encode_goaway_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;

        if let Payload::Goaway(_) = frame.payload() {
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());
            self.iterate_goaway_header(frame, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                self.state = FrameEncoderState::EncodingGoawayPayload;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_goaway_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, item) in buf.iter_mut().enumerate().take(len) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                0..=2 => {
                    if let Payload::Goaway(goaway_payload) = frame.payload() {
                        let payload_size = goaway_payload.encoded_len();
                        *item = ((payload_size >> (8 * (2 - header_byte_index))) & 0xFF) as u8;
                    } else {
                        return Err(FrameEncoderErr::UnexpectedPayloadType);
                    }
                }
                3 => {
                    *item = FrameType::Goaway as u8;
                }
                4 => {
                    *item = frame.flags().bits();
                }
                5..=8 => {
                    let stream_id_byte_index = header_byte_index - 5;
                    *item = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_goaway_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Goaway(goaway) = frame.payload() {
            let payload_size = goaway.encoded_len();
            let remaining_payload_bytes =
                payload_size.saturating_sub(self.encoded_bytes.saturating_sub(9));
            let bytes_to_write = remaining_payload_bytes.min(buf.len());

            self.iterate_goaway_payload(goaway, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == 9 + payload_size {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_goaway_payload(
        &self,
        goaway: &Goaway,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, buf_item) in buf.iter_mut().enumerate().take(len) {
            let payload_byte_index = self.encoded_bytes - 9 + buf_index;
            match payload_byte_index {
                0..=3 => {
                    let last_stream_id_byte_index = payload_byte_index;
                    *buf_item = (goaway.get_last_stream_id()
                        >> (24 - (8 * last_stream_id_byte_index)))
                        as u8;
                }
                4..=7 => {
                    let error_code_byte_index = payload_byte_index - 4;
                    *buf_item =
                        (goaway.get_error_code() >> (24 - (8 * error_code_byte_index))) as u8;
                }
                _ => {
                    let debug_data_index = payload_byte_index - 8;
                    if debug_data_index < goaway.get_debug_data().len() {
                        *buf_item = goaway.get_debug_data()[debug_data_index];
                    } else {
                        return Err(FrameEncoderErr::InternalError);
                    }
                }
            }
        }
        Ok(())
    }

    fn encode_window_update_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::WindowUpdate(_) = frame.payload() {
            // HTTP/2 frame header size is 9 bytes.
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());
            self.iterate_window_update_header(frame, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                self.state = FrameEncoderState::EncodingWindowUpdatePayload;
                // Resets the encoded_bytes counter here.
                self.encoded_bytes = 0;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_window_update_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, item) in buf.iter_mut().enumerate().take(len) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header. For
                // WindowUpdate frame, this is always 4 bytes.
                0..=1 => {
                    *item = 0;
                }
                2 => {
                    // Window Update frame payload size is always 4 bytes.
                    *item = 4;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    *item = FrameType::WindowUpdate as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    *item = frame.flags().bits();
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header.
                5..=8 => {
                    let stream_id_byte_index = header_byte_index - 5;
                    *item = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_window_update_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::WindowUpdate(window_update) = frame.payload() {
            let payload_size = 4usize;
            let remaining_payload_bytes =
                payload_size.saturating_sub(self.encoded_bytes.saturating_sub(9usize));
            let bytes_to_write = remaining_payload_bytes.min(buf.len());
            for (buf_index, buf_item) in buf.iter_mut().enumerate().take(bytes_to_write) {
                let payload_byte_index = self
                    .encoded_bytes
                    .saturating_sub(9)
                    .saturating_add(buf_index);
                let increment_byte_index = payload_byte_index;
                *buf_item =
                    (window_update.get_increment() >> (24 - (8 * increment_byte_index))) as u8;
            }
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == payload_size {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_settings_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Settings(settings) = frame.payload() {
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());
            self.iterate_settings_header(
                frame,
                buf,
                settings.get_settings().len() * 6,
                bytes_to_write,
            )?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                self.state = FrameEncoderState::EncodingSettingsPayload;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_settings_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        payload_len: usize,
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for buf_index in 0..len {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header.
                0..=2 => {
                    buf[buf_index] = ((payload_len >> (16 - (8 * buf_index))) & 0xFF) as u8;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    buf[3] = FrameType::Settings as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    buf[4] = frame.flags().bits();
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header. For SETTINGS frames, this should
                // always be 0.
                5..=8 => {
                    // Stream ID should be 0 for SETTINGS frames.
                    buf[buf_index] = 0;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_settings_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Settings(settings) = frame.payload() {
            let settings_len = settings.get_settings().len() * 6;
            let remaining_payload_bytes =
                settings_len.saturating_sub(self.encoded_bytes.saturating_sub(9));
            let bytes_to_write = remaining_payload_bytes.min(buf.len());
            self.iterate_settings_payload(settings, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == 9 + settings_len {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_settings_payload(
        &self,
        settings: &Settings,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, buf_item) in buf.iter_mut().enumerate().take(len) {
            let payload_byte_index = self.encoded_bytes - 9 + buf_index;
            let setting_index = payload_byte_index / 6;
            let setting_byte_index = payload_byte_index % 6;

            if let Some(setting) = settings.get_settings().get(setting_index) {
                let (id, value) = match setting {
                    Setting::HeaderTableSize(v) => (0x1, *v),
                    Setting::EnablePush(v) => (0x2, *v as u32),
                    Setting::MaxConcurrentStreams(v) => (0x3, *v),
                    Setting::InitialWindowSize(v) => (0x4, *v),
                    Setting::MaxFrameSize(v) => (0x5, *v),
                    Setting::MaxHeaderListSize(v) => (0x6, *v),
                };
                match setting_byte_index {
                    0..=1 => {
                        *buf_item = ((id >> (8 * (1 - setting_byte_index))) & 0xFF) as u8;
                    }
                    2..=5 => {
                        let shift_amount = 8 * (3 - (setting_byte_index - 2));
                        *buf_item = ((value >> shift_amount) & 0xFF) as u8;
                    }
                    _ => {
                        return Err(FrameEncoderErr::InternalError);
                    }
                }
            } else {
                return Err(FrameEncoderErr::InternalError);
            }
        }
        Ok(())
    }

    fn encode_priority_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Priority(_) = frame.payload() {
            // HTTP/2 frame header size is 9 bytes.
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());

            self.iterate_priority_header(frame, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                self.state = FrameEncoderState::EncodingPriorityPayload;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_priority_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, item) in buf.iter_mut().enumerate().take(len) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header.
                0..=2 => {
                    let payload_len = 5;
                    *item = ((payload_len >> (16 - (8 * header_byte_index))) & 0xFF) as u8;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    *item = frame.payload().frame_type() as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    *item = frame.flags().bits();
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header.
                5..=8 => {
                    let stream_id_byte_index = header_byte_index - 5;
                    *item = (frame.stream_id() >> (24 - (8 * stream_id_byte_index))) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_priority_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Priority(priority) = frame.payload() {
            // HTTP/2 frame header size is 9 bytes.
            let frame_header_size = 9;
            let remaining_payload_bytes = 5 - (self.encoded_bytes - frame_header_size);
            let bytes_to_write = remaining_payload_bytes.min(buf.len());

            self.iterate_priority_payload(priority, buf, frame_header_size, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size + 5 {
                self.state = FrameEncoderState::DataComplete
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_priority_payload(
        &self,
        priority: &Priority,
        buf: &mut [u8],
        frame_header_size: usize,
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for (buf_index, buf_item) in buf.iter_mut().enumerate().take(len) {
            let payload_byte_index = self
                .encoded_bytes
                .saturating_sub(frame_header_size)
                .saturating_add(buf_index);
            match payload_byte_index {
                0 => {
                    *buf_item = (priority.get_exclusive() as u8) << 7
                        | ((priority.get_stream_dependency() >> 24) & 0x7F) as u8;
                }
                1..=3 => {
                    let stream_dependency_byte_index = payload_byte_index - 1;
                    *buf_item = (priority.get_stream_dependency()
                        >> (16 - (8 * stream_dependency_byte_index)))
                        as u8;
                }
                4 => {
                    // The last byte is the weight.
                    *buf_item = priority.get_weight();
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_rst_stream_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        let frame_header_size = 9;
        if self.encoded_bytes >= frame_header_size {
            return Ok(0);
        }

        let bytes_to_write = (frame_header_size - self.encoded_bytes).min(buf.len());

        for (buf_index, item) in buf.iter_mut().enumerate().take(bytes_to_write) {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                0..=2 => {
                    let payload_len = 4;
                    *item = ((payload_len >> (16 - (8 * buf_index))) & 0xFF) as u8;
                }
                3 => {
                    *item = FrameType::RstStream as u8;
                }
                4 => {
                    *item = frame.flags().bits();
                }
                5..=8 => {
                    let stream_id = frame.stream_id();
                    *item = ((stream_id >> (24 - (8 * (buf_index - 5)))) & 0xFF) as u8;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        self.encoded_bytes += bytes_to_write;

        if self.encoded_bytes == frame_header_size {
            self.state = FrameEncoderState::EncodingRstStreamPayload;
        }

        Ok(bytes_to_write)
    }

    fn encode_rst_stream_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::RstStream(rst_stream) = frame.payload() {
            let frame_header_size = 9;
            if self.encoded_bytes < frame_header_size {
                return Ok(0);
            }

            let payload_size = 4;
            let encoded_payload_bytes = self.encoded_bytes - frame_header_size;

            if encoded_payload_bytes >= payload_size {
                return Ok(0);
            }

            let bytes_to_write = (payload_size - encoded_payload_bytes).min(buf.len());

            for (buf_index, item) in buf.iter_mut().enumerate().take(bytes_to_write) {
                let payload_byte_index = encoded_payload_bytes + buf_index;
                *item = ((rst_stream.error_code() >> (24 - (8 * payload_byte_index))) & 0xFF) as u8;
            }

            self.encoded_bytes += bytes_to_write;

            if self.encoded_bytes == frame_header_size + payload_size {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_ping_frame(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;
        if let Payload::Ping(_) = frame.payload() {
            let frame_header_size = 9;
            let remaining_header_bytes = if self.encoded_bytes >= frame_header_size {
                0
            } else {
                frame_header_size - self.encoded_bytes
            };
            let bytes_to_write = remaining_header_bytes.min(buf.len());
            self.iterate_ping_header(frame, buf, bytes_to_write)?;
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == frame_header_size {
                self.state = FrameEncoderState::EncodingPingPayload;
            }
            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn iterate_ping_header(
        &self,
        frame: &Frame,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FrameEncoderErr> {
        for buf_index in 0..len {
            let header_byte_index = self.encoded_bytes + buf_index;
            match header_byte_index {
                // The first 3 bytes represent the payload length in the frame header.
                0..=2 => {
                    // PING payload is always 8 bytes.
                    let payload_len = 8;
                    buf[buf_index] = ((payload_len >> (16 - (8 * buf_index))) & 0xFF) as u8;
                }
                // The 4th byte represents the frame type in the frame header.
                3 => {
                    buf[3] = FrameType::Ping as u8;
                }
                // The 5th byte represents the frame flags in the frame header.
                4 => {
                    buf[4] = frame.flags().bits();
                }
                // The last 4 bytes (6th to 9th) represent the stream identifier in the
                // frame header. For PING frames, this should always
                // be 0.
                5..=8 => {
                    // Stream ID should be 0 for PING frames.
                    buf[buf_index] = 0;
                }
                _ => {
                    return Err(FrameEncoderErr::InternalError);
                }
            }
        }
        Ok(())
    }

    fn encode_ping_payload(&mut self, buf: &mut [u8]) -> Result<usize, FrameEncoderErr> {
        let frame = self.try_retrieve_current_frame()?;

        if let Payload::Ping(ping) = frame.payload() {
            // PING payload is always 8 bytes.
            let payload_size = 8usize;
            let remaining_payload_bytes =
                payload_size.saturating_sub(self.encoded_bytes.saturating_sub(9usize));
            let bytes_to_write = remaining_payload_bytes.min(buf.len());
            for (buf_index, buf_item) in buf.iter_mut().enumerate().take(bytes_to_write) {
                let payload_byte_index = self
                    .encoded_bytes
                    .saturating_sub(9)
                    .saturating_add(buf_index);
                *buf_item = ping.data[payload_byte_index];
            }
            self.encoded_bytes += bytes_to_write;
            if self.encoded_bytes == 9 + 8 {
                self.state = FrameEncoderState::DataComplete;
            }

            Ok(bytes_to_write)
        } else {
            Err(FrameEncoderErr::UnexpectedPayloadType)
        }
    }

    fn encode_slice(&self, buf: &mut [u8], data: &[u8]) -> usize {
        let start = self.data_offset;
        let allow_to_write = (self.max_frame_size - start).min(self.remaining_payload_bytes);
        let bytes_to_write = allow_to_write.min(buf.len());

        buf[..bytes_to_write].copy_from_slice(&data[start..start + bytes_to_write]);
        bytes_to_write
    }

    // Helper method for writing the payload from the buffer to the output buffer.
    fn write_payload(&mut self, buf: &mut [u8], payload_len: usize) -> usize {
        let bytes_to_write = buf.len().min(payload_len - self.header_payload_index);
        buf[..bytes_to_write].copy_from_slice(
            &self.header_payload_buffer
                [self.header_payload_index..self.header_payload_index + bytes_to_write],
        );
        self.header_payload_index += bytes_to_write;
        bytes_to_write
    }
}

#[cfg(test)]
mod ut_frame_encoder {
    use super::*;
    use crate::error::HttpError;
    use crate::h2::frame::{
        Data, FrameFlags, Goaway, Headers, Ping, Priority, RstStream, Settings, WindowUpdate,
    };
    use crate::h2::{Parts, PseudoHeaders};

    /// UT test cases for `FrameEncoder` encoding `DATA` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Data`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_data_frame_encoding() {
        let mut encoder = FrameEncoder::new(4096, false);
        let data_payload = b"hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh".to_vec();

        let data_frame = Frame::new(
            131,
            FrameFlags::new(0),
            Payload::Data(Data::new(data_payload.clone())),
        );

        encoder.set_frame(data_frame).unwrap();

        let mut first_buf = [0u8; 2];
        let mut second_buf = [0u8; 38];

        let first_encoded = encoder.encode(&mut first_buf).unwrap();
        let second_encoded = encoder.encode(&mut second_buf).unwrap();

        assert_eq!(first_encoded, 2);
        assert_eq!(second_encoded, 38);
        assert_eq!(first_buf[0], 0);
        assert_eq!(first_buf[1], 0);
        assert_eq!(second_buf[0], data_payload.len() as u8);
        assert_eq!(second_buf[6], 131);

        for &item in second_buf.iter().skip(7).take(30) {
            assert_eq!(item, 104);
        }
    }

    /// UT test cases for `FrameEncoder` encoding `HEADERS` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Headers`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_headers_frame_encoding() {
        let mut frame_encoder = FrameEncoder::new(4096, false);

        let mut new_parts = Parts::new();
        new_parts.pseudo.set_method(Some("GET".to_string()));
        new_parts.pseudo.set_scheme(Some("https".to_string()));
        new_parts.pseudo.set_path(Some("/code".to_string()));
        new_parts
            .pseudo
            .set_authority(Some("example.com".to_string()));
        let mut frame_flag = FrameFlags::empty();
        frame_flag.set_end_headers(true);
        frame_flag.set_end_stream(true);
        let frame = Frame::new(1, frame_flag, Payload::Headers(Headers::new(new_parts)));

        // Set the current frame for the encoder
        frame_encoder.set_frame(frame).unwrap();

        let mut buf = vec![0; 50];
        let first_encoded = frame_encoder.encode(&mut buf).unwrap();
        assert_eq!(first_encoded, 22 + 9);
        assert_eq!(buf[0], 0);
        assert_eq!(buf[2], 22);
        assert_eq!(buf[3], 0x1);
        assert_eq!(buf[4], 5);
        assert_eq!(buf[8], 1);

        assert_eq!(frame_encoder.state, FrameEncoderState::HeadersComplete);
    }

    /// UT test cases for `FrameEncoder` encoding `SETTINGS` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Settings`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_settings_frame_encoding() {
        let mut encoder = FrameEncoder::new(4096, false);
        let settings_payload = vec![
            Setting::HeaderTableSize(4096),
            Setting::EnablePush(true),
            Setting::MaxConcurrentStreams(100),
            Setting::InitialWindowSize(65535),
            Setting::MaxFrameSize(16384),
            Setting::MaxHeaderListSize(8192),
        ];

        let settings = Settings::new(settings_payload.clone());

        let settings_frame = Frame::new(0, FrameFlags::new(0), Payload::Settings(settings));

        let mut first_buf = [0u8; 9];
        let mut second_buf = [0u8; 30];
        let mut third_buf = [0u8; 6];
        encoder.set_frame(settings_frame).unwrap();

        let first_encoded = encoder.encode(&mut first_buf).unwrap();
        assert_eq!(encoder.state, FrameEncoderState::EncodingSettingsPayload);
        let second_encoded = encoder.encode(&mut second_buf).unwrap();
        let third_encoded = encoder.encode(&mut third_buf).unwrap();

        assert_eq!(encoder.state, FrameEncoderState::DataComplete);
        // Updated expected value for first_encoded
        assert_eq!(first_encoded, 9);
        assert_eq!(second_encoded, 30);
        // Updated expected value for third_encoded
        assert_eq!(third_encoded, 6);

        // Validate the encoded settings
        let mut expected_encoded_settings = [0u8; 60];
        for (i, setting) in settings_payload.iter().enumerate() {
            let offset = i * 6;
            let (id, value) = match setting {
                Setting::HeaderTableSize(v) => (0x1, *v),
                Setting::EnablePush(v) => (0x2, *v as u32),
                Setting::MaxConcurrentStreams(v) => (0x3, *v),
                Setting::InitialWindowSize(v) => (0x4, *v),
                Setting::MaxFrameSize(v) => (0x5, *v),
                Setting::MaxHeaderListSize(v) => (0x6, *v),
            };
            expected_encoded_settings[offset] = (id >> 8) as u8;
            expected_encoded_settings[offset + 1] = (id & 0xFF) as u8;
            expected_encoded_settings[offset + 2] = (value >> 24) as u8;
            expected_encoded_settings[offset + 3] = ((value >> 16) & 0xFF) as u8;
            expected_encoded_settings[offset + 4] = ((value >> 8) & 0xFF) as u8;
            expected_encoded_settings[offset + 5] = (value & 0xFF) as u8;
        }

        let actual_encoded_settings = [&second_buf[..], &third_buf[..]].concat();
        for i in 0..35 {
            assert_eq!(expected_encoded_settings[i], actual_encoded_settings[i]);
        }
    }

    /// UT test cases for `FrameEncoder` encoding `PING` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Ping`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_ping_frame_encoding() {
        let mut encoder = FrameEncoder::new(4096, false);
        let ping_payload = [1, 2, 3, 4, 5, 6, 7, 8];

        let ping_frame = Frame::new(
            0,
            FrameFlags::new(0),
            Payload::Ping(Ping { data: ping_payload }),
        );

        encoder.set_frame(ping_frame).unwrap();

        let mut first_buf = [0u8; 9];
        let mut second_buf = [0u8; 8];

        let first_encoded = encoder.encode(&mut first_buf).unwrap();
        let second_encoded = encoder.encode(&mut second_buf).unwrap();

        assert_eq!(first_encoded, 9);
        assert_eq!(second_encoded, 8);

        assert_eq!(first_buf[0], 0);
        assert_eq!(first_buf[1], 0);
        // payload length
        assert_eq!(first_buf[2], 8);
        assert_eq!(first_buf[3], FrameType::Ping as u8);
        // flags
        assert_eq!(first_buf[4], 0);
        // stream id
        assert_eq!(first_buf[5], 0);
        // stream id
        assert_eq!(first_buf[6], 0);
        // stream id
        assert_eq!(first_buf[7], 0);
        // stream id
        assert_eq!(first_buf[8], 0);

        for i in 0..8 {
            assert_eq!(second_buf[i], ping_payload[i]);
        }
    }

    /// UT test case for FrameEncoder encoding a sequence of frames: Headers,
    /// Data, Headers.
    ///
    /// # Brief
    /// 1. Creates a FrameEncoder.
    /// 2. Creates multiple frames including Headers and Data frames.
    /// 3. Sets each frame for the encoder and encodes them using buffer
    ///    segments.
    /// 4. Checks whether the encoding results are correct.
    #[test]
    fn ut_continue_frame_encoding() {
        let mut encoder = FrameEncoder::new(4096, false);

        let mut new_parts = Parts::new();
        new_parts.pseudo.set_method(Some("GET".to_string()));
        new_parts.pseudo.set_scheme(Some("https".to_string()));
        new_parts.pseudo.set_path(Some("/code".to_string()));
        new_parts
            .pseudo
            .set_authority(Some("example.com".to_string()));
        let mut frame_flag = FrameFlags::empty();
        frame_flag.set_end_headers(true);
        frame_flag.set_end_stream(false);
        let frame_1 = Frame::new(
            1,
            frame_flag.clone(),
            Payload::Headers(Headers::new(new_parts.clone())),
        );

        let data_payload = b"hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh".to_vec();
        let data_frame = Frame::new(
            1,
            FrameFlags::new(1),
            Payload::Data(Data::new(data_payload)),
        );

        let frame_2 = Frame::new(
            1,
            frame_flag.clone(),
            Payload::Headers(Headers::new(new_parts.clone())),
        );

        encoder.set_frame(frame_1).unwrap();
        let mut first_buf = [0u8; 50];
        let first_encoding = encoder.encode(&mut first_buf).unwrap();

        encoder.set_frame(data_frame).unwrap();
        let mut second_buf = [0u8; 50];
        let second_encoding = encoder.encode(&mut second_buf).unwrap();

        encoder.set_frame(frame_2).unwrap();
        let mut third_buf = [0u8; 50];
        let third_encoding = encoder.encode(&mut third_buf).unwrap();

        assert_eq!(first_encoding, 31);
        assert_eq!(second_encoding, 40);
        assert_eq!(third_encoding, 13);

        assert_eq!(first_buf[2], 22);
        assert_eq!(second_buf[2], 31);
        assert_eq!(third_buf[2], 4);
    }

    /// UT test cases for `FrameEncoder` encoding `RST_STREAM` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::RstStream`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_rst_stream_frame_encoding() {
        let mut frame_encoder = FrameEncoder::new(4096, false);

        let error_code = 12345678;
        let rst_stream_payload = Payload::RstStream(RstStream::new(error_code));

        let frame_flags = FrameFlags::empty();
        let frame = Frame::new(
            // Stream ID can be non-zero for RST_STREAM frames
            1,
            frame_flags,
            rst_stream_payload,
        );

        // Set the current frame for the encoder
        frame_encoder.set_frame(frame).unwrap();

        let mut buf = vec![0; 50];
        let first_encoded = frame_encoder.encode(&mut buf).unwrap();
        // 9 bytes for header, 4 bytes for payload
        assert_eq!(first_encoded, 9 + 4);
        assert_eq!(buf[0], 0);
        // payload length should be 4 for RST_STREAM frames
        assert_eq!(buf[2], 4);
        assert_eq!(buf[3], FrameType::RstStream as u8);
        // frame flags should be 0 for RST_STREAM frames
        assert_eq!(buf[4], 0);
        // stream ID should be 1 for this test case
        assert_eq!(buf[8], 1);

        // Check if encoded error code is correct
        assert_eq!(&buf[9..13], &error_code.to_be_bytes());

        assert_eq!(frame_encoder.state, FrameEncoderState::DataComplete);
    }

    /// UT test cases for `FrameEncoder` encoding `WINDOW_UPDATE` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::WindowUpdate`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_window_update_frame_encoding() {
        let mut frame_encoder = FrameEncoder::new(4096, false);

        let window_size_increment = 12345678;
        let window_update_payload = Payload::WindowUpdate(WindowUpdate::new(window_size_increment));

        let frame_flags = FrameFlags::empty();
        let frame = Frame::new(
            // Stream ID can be zero for WINDOW_UPDATE frames.
            0,
            frame_flags,
            window_update_payload,
        );

        // Sets the current frame for the encoder.
        frame_encoder.set_frame(frame).unwrap();

        let mut buf = vec![0; 50];
        let first_encoded = frame_encoder.encode(&mut buf).unwrap();
        // 9 bytes for header, 4 bytes for payload.
        assert_eq!(first_encoded, 9 + 4);
        assert_eq!(buf[0], 0);
        // Payload length should be 4 for WINDOW_UPDATE frames.
        assert_eq!(buf[2], 4);
        assert_eq!(buf[3], FrameType::WindowUpdate as u8);
        // Frame flags should be 0 for WINDOW_UPDATE frames.
        assert_eq!(buf[4], 0);
        // Stream ID should be 0 for this test case.
        assert_eq!(buf[8], 0);

        // Checks if encoded window size increment is correct.
        assert_eq!(&buf[9..13], &window_size_increment.to_be_bytes());

        assert_eq!(frame_encoder.state, FrameEncoderState::DataComplete);
    }

    /// UT test case for FrameEncoder encoding `PRIORITY` frame.
    ///
    /// # Brief
    /// 1. Creates a FrameEncoder.
    /// 2. Creates a Frame with Payload::Priority.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_priority_frame_encoding() {
        let mut encoder = FrameEncoder::new(4096, false);
        // Maximum value for a 31-bit integer
        let stream_dependency = 0x7FFFFFFF;
        let priority_payload = Priority::new(true, stream_dependency, 15);

        let priority_frame =
            Frame::new(131, FrameFlags::new(0), Payload::Priority(priority_payload));

        encoder.set_frame(priority_frame).unwrap();

        let mut buf = [0u8; 14];

        let encoded = encoder.encode(&mut buf).unwrap();

        assert_eq!(encoded, 14);
        // Payload length (most significant byte)
        assert_eq!(buf[0], 0);
        // Payload length (middle byte)
        assert_eq!(buf[1], 0);
        // Payload length (least significant byte)
        assert_eq!(buf[2], 5);
        // Frame flags
        assert_eq!(buf[3], FrameType::Priority as u8);
        assert_eq!(buf[4], 0);
        // Stream ID (most significant byte)
        assert_eq!(buf[5], 0);
        // Stream ID (middle bytes)
        assert_eq!(buf[6], 0);
        // Stream ID (middle bytes)
        assert_eq!(buf[7], 0);
        // Stream ID (least significant byte)
        assert_eq!(buf[8], 131);
        // Exclusive flag and most significant byte of stream dependency
        assert_eq!(buf[9], (0x80 | ((stream_dependency >> 24) & 0x7F)) as u8);
        // Stream dependency (middle bytes)
        assert_eq!(buf[10], ((stream_dependency >> 16) & 0xFF) as u8);
        // Stream dependency (middle bytes)
        assert_eq!(buf[11], ((stream_dependency >> 8) & 0xFF) as u8);
        // Stream dependency (least significant byte)
        assert_eq!(buf[12], (stream_dependency & 0xFF) as u8);
        // Weight
        assert_eq!(buf[13], 15);
    }

    /// UT test cases for `FrameEncoder` encoding `GOAWAY` frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Goaway`.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the frame and its payload using buffer segments.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_goaway_frame_encoding() {
        // 1. Creates a `FrameEncoder`.
        let mut encoder = FrameEncoder::new(4096, false);

        // 2. Creates a `Frame` with `Payload::Goaway`.
        let last_stream_id = 1;
        let error_code = 1;
        let debug_data = vec![1, 2, 3, 4, 5];
        let goaway_frame = Frame::new(
            131,
            FrameFlags::new(0),
            Payload::Goaway(Goaway::new(error_code, last_stream_id, debug_data.clone())),
        );

        // 3. Sets the frame for the encoder.
        encoder.set_frame(goaway_frame).unwrap();

        // 4. Encodes the frame and its payload using buffer segments.
        let mut first_buf = [0u8; 9];
        let mut second_buf = [0u8; 13];
        let first_encoded = encoder.encode(&mut first_buf).unwrap();
        let second_encoded = encoder.encode(&mut second_buf).unwrap();

        // 5. Checks whether the result is correct.
        assert_eq!(first_encoded, 9);
        assert_eq!(second_encoded, 13);

        // Validate the encoded GOAWAY frame.
        let mut expected_encoded_goaway = [0u8; 13];
        expected_encoded_goaway[0..4].copy_from_slice(&last_stream_id.to_be_bytes());
        expected_encoded_goaway[4..8].copy_from_slice(&error_code.to_be_bytes());

        expected_encoded_goaway[8..13].copy_from_slice(&debug_data[..]);

        // payload length should be 13 bytes
        assert_eq!(first_buf[0..3], [0u8, 0, 13]);
        assert_eq!(first_buf[3], FrameType::Goaway as u8);
        // flags
        assert_eq!(first_buf[4], 0);

        // Validate the encoded Last-Stream-ID, Error Code, and debug data
        assert_eq!(second_buf[..], expected_encoded_goaway[..]);
    }

    /// UT test cases for `FrameEncoder::update_max_frame_size`.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Updates the maximum frame size.
    /// 3. Checks whether the maximum frame size was updated correctly.
    #[test]
    fn ut_update_max_frame_size() {
        let mut encoder = FrameEncoder::new(4096, false);
        encoder.update_max_frame_size(8192);
        assert_eq!(encoder.max_frame_size, 8192);
    }

    /// UT test cases for `FrameEncoder::update_setting`.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Setting` variant.
    /// 3. Creates a `Frame` with `Payload::Settings`.
    /// 4. Sets the frame for the encoder.
    /// 5. Updates the setting.
    /// 6. Checks whether the setting was updated correctly.
    #[test]
    fn ut_update_setting() {
        let mut encoder = FrameEncoder::new(4096, false);
        let settings_payload = vec![Setting::MaxFrameSize(4096)];
        let settings = Settings::new(settings_payload);
        let settings_frame = Frame::new(0, FrameFlags::new(0), Payload::Settings(settings));

        encoder.set_frame(settings_frame).unwrap();
        let new_setting = Setting::MaxFrameSize(8192);
        encoder.update_setting(new_setting.clone());

        if let Some(frame) = &mut encoder.current_frame {
            if let Payload::Settings(settings) = frame.payload_mut() {
                let updated_settings = settings.get_settings();
                assert!(updated_settings.iter().any(|s| *s == new_setting));
            }
        }
    }

    /// UT test cases for `FrameEncoder` encoding continuation frames.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Headers` and sets the flags.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the continuation frames using a buffer.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encode_continuation_frames() {
        let mut frame_encoder = FrameEncoder::new(4096, false);
        let mut new_parts = Parts::new();
        assert!(new_parts.is_empty());
        new_parts.pseudo.set_method(Some("GET".to_string()));
        new_parts.pseudo.set_scheme(Some("https".to_string()));
        new_parts.pseudo.set_path(Some("/code".to_string()));
        new_parts
            .pseudo
            .set_authority(Some("example.com".to_string()));

        let mut frame_flag = FrameFlags::empty();
        frame_flag.set_end_headers(true);
        frame_flag.set_end_stream(false);
        let frame = Frame::new(
            1,
            frame_flag.clone(),
            Payload::Headers(Headers::new(new_parts.clone())),
        );

        frame_encoder.set_frame(frame).unwrap();
        frame_encoder.state = FrameEncoderState::EncodingContinuationFrames;
        let mut buf = [0u8; 5000];

        assert!(frame_encoder.encode_continuation_frames(&mut buf).is_ok());

        let mut frame_flag = FrameFlags::empty();
        frame_flag.set_end_headers(true);
        let frame = Frame::new(
            1,
            frame_flag,
            Payload::Headers(Headers::new(new_parts.clone())),
        );

        frame_encoder.set_frame(frame).unwrap();
        frame_encoder.state = FrameEncoderState::EncodingContinuationFrames;
        assert!(frame_encoder.encode_continuation_frames(&mut buf).is_ok());

        let mut frame_flag = FrameFlags::empty();
        frame_flag.set_end_headers(true);
        let frame = Frame::new(1, frame_flag, Payload::Ping(Ping::new([0; 8])));

        frame_encoder.set_frame(frame).unwrap();
        frame_encoder.state = FrameEncoderState::EncodingContinuationFrames;
        assert!(frame_encoder.encode_continuation_frames(&mut buf).is_err());
    }

    /// UT test cases for `FrameEncoder` encoding padded data.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` with `Payload::Data` and sets the flags.
    /// 3. Sets the frame for the encoder.
    /// 4. Encodes the padding using a buffer.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encode_padding() {
        let mut frame_encoder = FrameEncoder::new(4096, false);

        // Creates a padded data frame.
        let mut frame_flags = FrameFlags::empty();
        frame_flags.set_end_headers(true);
        frame_flags.set_padded(true);
        let data_payload = vec![0u8; 500];
        let data_frame = Frame::new(
            1,
            frame_flags.clone(),
            Payload::Data(Data::new(data_payload)),
        );

        // Sets the frame to the frame_encoder and test padding encoding.
        frame_encoder.set_frame(data_frame).unwrap();
        frame_encoder.state = FrameEncoderState::EncodingDataPadding;
        let mut buf = [0u8; 600];
        assert!(frame_encoder.encode_padding(&mut buf).is_ok());

        let headers_payload = Payload::Headers(Headers::new(Parts::new()));
        let headers_frame = Frame::new(1, frame_flags.clone(), headers_payload);
        frame_encoder.set_frame(headers_frame).unwrap();
        frame_encoder.state = FrameEncoderState::EncodingDataPadding;
        assert!(frame_encoder.encode_padding(&mut buf).is_err());

        frame_encoder.current_frame = None;
        assert!(frame_encoder.encode_padding(&mut buf).is_err());
    }

    /// UT test cases for `FrameEncoder` encoding data frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` smaller than max_frame_size with `Payload::Data`
    ///    and sets the flags.
    /// 3. Sets the frame for the encoder.
    /// 4. Encode the data frame with a buf.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encode_small_data_frame() {
        let mut encoder = FrameEncoder::new(100, false);
        let data_payload = vec![b'a'; 10];
        let mut buf = [0u8; 10];
        encode_small_frame(&mut encoder, &mut buf, data_payload.clone());

        // buf is larger than frame but smaller than max_frame_size;
        let mut buf = [0u8; 50];
        encode_small_frame(&mut encoder, &mut buf, data_payload.clone());

        // buf is larger than max_frame_size;
        let mut buf = [0u8; 200];
        encode_small_frame(&mut encoder, &mut buf, data_payload);
    }

    fn encode_small_frame(encoder: &mut FrameEncoder, buf: &mut [u8], data_payload: Vec<u8>) {
        let data_frame = Frame::new(
            1,
            FrameFlags::new(1),
            Payload::Data(Data::new(data_payload)),
        );
        encoder.set_frame(data_frame).unwrap();

        let mut result = [b'0'; 10 + 9];
        let total = assert_encoded_data(encoder, &mut result, buf);
        assert_eq!(total, 10 + 9);
        assert_eq!(result[4], 0x1);
        assert_eq!(encoder.state, FrameEncoderState::DataComplete);
    }

    /// UT test cases for `FrameEncoder` encoding data frame.
    ///
    /// # Brief
    /// 1. Creates a `FrameEncoder`.
    /// 2. Creates a `Frame` larger than max_frame_size with `Payload::Data` and
    ///    sets the flags.
    /// 3. Sets the frame for the encoder.
    /// 4. Encode the data frame with a buf.
    /// 5. Checks whether the result is correct.
    #[test]
    fn ut_encode_large_data_frame() {
        let mut encoder = FrameEncoder::new(100, false);
        let data_payload = vec![b'a'; 1024];
        let mut buf = [0u8; 10];

        let data_frame = Frame::new(
            1,
            FrameFlags::new(0),
            Payload::Data(Data::new(data_payload.clone())),
        );
        let mut result = [b'0'; 1024 + 9 * 11];
        encoder.set_frame(data_frame).unwrap();

        let total = assert_encoded_data(&mut encoder, &mut result, &mut buf);
        // This is because the data frame flag is 0.
        assert_eq!(result[4 + 10 * (9 + 100)], 0x0);
        assert_eq!(total, 1024 + 9 * 11);
        for index in 0..=9 {
            assert_eq!(result[4 + index * (9 + 100)], 0x0);
        }
        assert_eq!(encoder.state, FrameEncoderState::DataComplete);

        // finished
        let data_frame = Frame::new(
            1,
            FrameFlags::new(1),
            Payload::Data(Data::new(data_payload)),
        );
        let mut result = [b'0'; 1024 + 9 * 11];
        encoder.set_frame(data_frame).unwrap();

        let total = assert_encoded_data(&mut encoder, &mut result, &mut buf);
        // This is because the data frame flag is 0.
        assert_eq!(result[4 + 10 * (9 + 100)], 0x1);
        assert_eq!(total, 1024 + 9 * 11);
        for index in 0..=9 {
            assert_eq!(result[4 + index * (9 + 100)], 0x0);
        }
        assert_eq!(encoder.state, FrameEncoderState::DataComplete);
    }

    fn assert_encoded_data(encoder: &mut FrameEncoder, result: &mut [u8], buf: &mut [u8]) -> usize {
        let mut total = 0;
        loop {
            let size = encoder.encode(buf).unwrap();
            result[total..total + size].copy_from_slice(&buf[..size]);
            total += size;
            if size == 0 {
                break;
            }
        }
        total
    }
}
