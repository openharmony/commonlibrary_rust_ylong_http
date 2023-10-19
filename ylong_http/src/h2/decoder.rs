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

use std::ops::{Deref, DerefMut};

use super::{Frame, H2Error};
use crate::error::ErrorKind::H2;
use crate::h2;
use crate::h2::decoder::Stage::{Header, Payload};
use crate::h2::error::ErrorCode;
use crate::h2::frame::{
    Data, FrameFlags, Goaway, Ping, Priority, RstStream, WindowUpdate, ACK_MASK, END_HEADERS_MASK,
    END_STREAM_MASK, HEADERS_PRIORITY_MASK, PADDED_MASK,
};
use crate::h2::{frame, HpackDecoder, Parts, Setting, Settings};
use crate::headers::Headers;

const FRAME_HEADER_LENGTH: usize = 9;
const DEFAULT_MAX_FRAME_SIZE: u32 = 2 << 13;
const MAX_ALLOWED_MAX_FRAME_SIZE: u32 = (2 << 23) - 1;
const DEFAULT_HEADER_TABLE_SIZE: usize = 4096;
const DEFAULT_MAX_HEADER_LIST_SIZE: usize = 16 << 20;
const MAX_INITIAL_WINDOW_SIZE: usize = (1 << 31) - 1;

/// A set of consecutive Frames.
/// When Headers Frames or Continuation Frames are not End Headers, they are
/// represented as `FrameKind::Partial`.
///
/// - use `Frames` iterator.
///
/// # Examples
///
/// ```
/// use ylong_http::h2::Frames;
///
/// # fn get_frames_iter(frames: Frames) {
/// let mut iter = frames.iter();
/// let next_frame = iter.next();
/// # }
/// ```
///
/// - use `Frames` consuming iterator.
///
/// # Examples
///
/// ```
/// use ylong_http::h2::Frames;
///
/// # fn get_frames_into_iter(frames: Frames) {
/// let mut iter = frames.into_iter();
/// let next_frame = iter.next();
/// # }
/// ```
pub struct Frames {
    list: Vec<FrameKind>,
}

/// An iterator of `Frames`.
pub struct FramesIter<'a> {
    iter: core::slice::Iter<'a, FrameKind>,
}

/// A consuming iterator of `Frames`.
pub struct FramesIntoIter {
    into_iter: std::vec::IntoIter<FrameKind>,
}

impl Frames {
    /// Returns an iterator over `Frames`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h2::Frames;
    ///
    /// # fn get_frames_iter(frames: Frames) {
    /// let mut iter = frames.iter();
    /// let next_frame = iter.next();
    /// # }
    /// ```
    pub fn iter(&self) -> FramesIter {
        FramesIter {
            iter: self.list.iter(),
        }
    }

    /// Returns the size of `Frames`.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Checks if the `Frames` is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a> Deref for FramesIter<'a> {
    type Target = core::slice::Iter<'a, FrameKind>;

    fn deref(&self) -> &Self::Target {
        &self.iter
    }
}

impl<'a> DerefMut for FramesIter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.iter
    }
}

// TODO Added the Iterator trait implementation of ChunksIter.
impl<'a> Iterator for FramesIter<'a> {
    type Item = &'a FrameKind;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl Iterator for FramesIntoIter {
    type Item = FrameKind;

    fn next(&mut self) -> Option<Self::Item> {
        self.into_iter.next()
    }
}

impl core::iter::IntoIterator for Frames {
    type Item = FrameKind;
    type IntoIter = FramesIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        FramesIntoIter {
            into_iter: self.list.into_iter(),
        }
    }
}

/// When Headers Frames or Continuation Frames are not End Headers, they are
/// represented as `FrameKind::Partial`.
pub enum FrameKind {
    /// PUSH_PROMISE or HEADRS frame parsing completed.
    Complete(Frame),
    /// Partial decoded of PUSH_PROMISE or HEADRS frame.
    Partial,
}

/// Frame bytes sequence decoder, supporting fragment deserialization of Frames.
///
/// # Examples
///
/// ```
/// use ylong_http::h2::FrameDecoder;
///
/// let mut decoder = FrameDecoder::new();
/// decoder.set_max_header_list_size(30);
/// let data_frame_bytes = &[0, 0, 5, 0, 0, 0, 0, 0, 1, b'h', b'e', b'l', b'l', b'o'];
/// let decoded_frames = decoder.decode(data_frame_bytes).unwrap();
/// let frame_kind = decoded_frames.iter().next().unwrap();
/// ```
pub struct FrameDecoder {
    buffer: Vec<u8>,
    // buffer's length
    offset: usize,
    max_frame_size: u32,
    // Current decode Stage of decoder
    stage: Stage,
    // 9-byte header information of the current frame
    header: FrameHeader,
    hpack: HpackDecoderLayer,
    // The Headers Frame flags information is saved to ensure the continuity between Headers Frames
    // and Continuation Frames.
    continuations: Continuations,
}

enum Stage {
    Header,
    Payload,
}

struct HpackDecoderLayer {
    hpack: HpackDecoder,
}

#[derive(Default)]
struct FrameHeader {
    stream_id: usize,
    flags: u8,
    frame_type: u8,
    payload_length: usize,
}

struct Continuations {
    flags: u8,
    is_end_stream: bool,
    stream_id: usize,
    is_end_headers: bool,
    promised_stream_id: u32,
}

impl HpackDecoderLayer {
    fn new() -> Self {
        Self {
            hpack: HpackDecoder::with_max_size(
                DEFAULT_HEADER_TABLE_SIZE,
                DEFAULT_MAX_HEADER_LIST_SIZE,
            ),
        }
    }

    fn hpack_decode(&mut self, buf: &[u8]) -> Result<(), H2Error> {
        self.hpack.decode(buf)
    }

    fn hpack_finish(&mut self) -> Result<Parts, H2Error> {
        self.hpack.finish()
    }

    pub fn set_max_header_list_size(&mut self, size: usize) {
        self.hpack.update_header_list_size(size)
    }
}

impl FrameHeader {
    fn new() -> Self {
        FrameHeader::default()
    }

    fn reset(&mut self) {
        self.stream_id = 0;
        self.flags = 0;
        self.frame_type = 0;
        self.payload_length = 0
    }

    fn is_end_stream(&self) -> bool {
        END_STREAM_MASK & self.flags == END_STREAM_MASK
    }

    fn is_padded(&self) -> bool {
        PADDED_MASK & self.flags == PADDED_MASK
    }

    fn is_end_headers(&self) -> bool {
        END_HEADERS_MASK & self.flags == END_HEADERS_MASK
    }

    fn is_headers_priority(&self) -> bool {
        HEADERS_PRIORITY_MASK & self.flags == HEADERS_PRIORITY_MASK
    }

    fn is_ack(&self) -> bool {
        ACK_MASK & self.flags == ACK_MASK
    }
}

impl Continuations {
    fn new() -> Self {
        Continuations {
            flags: 0,
            is_end_stream: false,
            stream_id: 0,
            // The initial value is true.
            is_end_headers: true,
            promised_stream_id: 0,
        }
    }

    fn reset(&mut self) {
        self.flags = 0;
        self.is_end_stream = false;
        self.is_end_headers = true;
        self.stream_id = 0;
        self.promised_stream_id = 0;
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        FrameDecoder {
            buffer: vec![],
            offset: 0,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            stage: Stage::Header,
            header: FrameHeader::new(),
            hpack: HpackDecoderLayer::new(),
            continuations: Continuations::new(),
        }
    }
}

impl Frames {
    fn new() -> Self {
        Frames { list: vec![] }
    }
    fn push(&mut self, frame: FrameKind) {
        self.list.push(frame)
    }
}

impl FrameDecoder {
    /// `FrameDecoder` constructor. Three parameters are defined in SETTINGS
    /// Frame.
    pub fn new() -> Self {
        FrameDecoder::default()
    }

    /// Updates the SETTINGS_MAX_FRAME_SIZE.
    pub fn set_max_frame_size(&mut self, size: u32) -> Result<(), H2Error> {
        if !(DEFAULT_MAX_FRAME_SIZE..=MAX_ALLOWED_MAX_FRAME_SIZE).contains(&size) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        self.max_frame_size = size;
        Ok(())
    }

    /// Updates the SETTINGS_MAX_HEADER_LIST_SIZE.
    pub fn set_max_header_list_size(&mut self, size: usize) {
        self.hpack.set_max_header_list_size(size)
    }

    /// Frames deserialization interface, supporting segment decode.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h2::FrameDecoder;
    ///
    /// let mut decoder = FrameDecoder::new();
    /// decoder.set_max_header_list_size(30);
    /// let data_frame_bytes = &[0, 0, 5, 0, 0, 0, 0, 0, 1, b'h', b'e', b'l', b'l', b'o'];
    /// let decoded_frames = decoder.decode(&data_frame_bytes[..9]).unwrap();
    /// assert_eq!(decoded_frames.len(), 0);
    /// let decoded_frames = decoder.decode(&data_frame_bytes[9..]).unwrap();
    /// assert_eq!(decoded_frames.len(), 1);
    /// ```
    pub fn decode(&mut self, buf: &[u8]) -> Result<Frames, H2Error> {
        let mut frames = Frames::new();
        let mut buffer = buf;
        loop {
            match self.stage {
                Header => match self.decode_frame_header(buffer)? {
                    Some(remain) => {
                        buffer = remain;
                        self.stage = Payload;
                    }
                    None => {
                        break;
                    }
                },
                Payload => match self.decode_frame_payload(buffer)? {
                    Some((remain, frame)) => {
                        frames.push(frame);
                        buffer = remain;
                        self.stage = Header;
                    }
                    None => {
                        break;
                    }
                },
            }
        }
        Ok(frames)
    }

    fn decode_frame_payload<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<Option<(&'a [u8], FrameKind)>, H2Error> {
        // Frames of other types or streams are not allowed between Headers Frame and
        // Continuation Frame.
        if !self.continuations.is_end_headers
            && (self.header.stream_id != self.continuations.stream_id
                || self.header.frame_type != 9)
        {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }

        let frame_end_index = self.header.payload_length - self.offset;
        if buf.len() < frame_end_index {
            self.offset += buf.len();
            self.buffer.extend_from_slice(buf);
            Ok(None)
        } else {
            let frame = match self.header.frame_type {
                0 => self.decode_data_payload(&buf[..frame_end_index])?,
                1 => self.decode_headers_payload(&buf[..frame_end_index])?,
                2 => self.decode_priority_payload(&buf[..frame_end_index])?,
                3 => self.decode_reset_payload(&buf[..frame_end_index])?,
                4 => self.decode_settings_payload(&buf[..frame_end_index])?,
                5 => self.decode_push_promise_payload(&buf[..frame_end_index])?,
                6 => self.decode_ping_payload(&buf[..frame_end_index])?,
                7 => self.decode_goaway_payload(&buf[..frame_end_index])?,
                8 => self.decode_window_update_payload(&buf[..frame_end_index])?,
                9 => self.decode_continuation_payload(&buf[..frame_end_index])?,
                _ => {
                    return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                }
            };
            self.header.reset();
            if self.offset != 0 {
                self.offset = 0;
                self.buffer.clear()
            }
            Ok(Some((&buf[frame_end_index..], frame)))
        }
    }

    fn decode_ping_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if !is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length != 8 || buf.len() != 8 {
            return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
        }
        let mut opaque_data = [0; 8];
        opaque_data.copy_from_slice(buf);
        let ping = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::Ping(Ping::new(opaque_data)),
        );
        Ok(FrameKind::Complete(ping))
    }

    fn decode_priority_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        const EXCLUSIVE_MASK: u8 = 0x80;

        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        if self.header.payload_length != 5 || buf.len() != 5 {
            return Err(H2Error::StreamError(
                self.header.stream_id as u32,
                ErrorCode::FrameSizeError,
            ));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        let exclusive = buf[0] & EXCLUSIVE_MASK == EXCLUSIVE_MASK;
        let stream_dependency = get_stream_id(&buf[..4]);
        let weight = buf[4];
        let priority = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::Priority(Priority::new(exclusive, stream_dependency, weight)),
        );
        Ok(FrameKind::Complete(priority))
    }

    fn decode_goaway_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if !is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length < 8 || buf.len() < 8 {
            return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
        }
        let last_stream_id = get_stream_id(&buf[..4]) as usize;
        let error_code = get_code_value(&buf[4..8]);
        let mut debug_data = vec![];
        debug_data.extend_from_slice(&buf[8..]);
        let goaway = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::Goaway(Goaway::new(error_code, last_stream_id, debug_data)),
        );
        Ok(FrameKind::Complete(goaway))
    }

    // window_update frame contains stream frame and connection frame
    fn decode_window_update_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length != 4 || buf.len() != 4 {
            return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
        }
        let increment_size = (((0x7f & buf[0]) as u32) << 24)
            | ((buf[1] as u32) << 16)
            | ((buf[2] as u32) << 8)
            | (buf[3] as u32);
        if increment_size == 0 {
            return Err(H2Error::StreamError(
                self.header.stream_id as u32,
                ErrorCode::ProtocolError,
            ));
        }
        let window_update = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::WindowUpdate(WindowUpdate::new(increment_size)),
        );
        Ok(FrameKind::Complete(window_update))
    }

    fn decode_reset_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length != 4 || buf.len() != 4 {
            return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
        }
        let code = get_code_value(&buf[..4]);
        let reset = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::RstStream(RstStream::new(code)),
        );
        Ok(FrameKind::Complete(reset))
    }

    fn decode_settings_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if !is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length % 6 != 0 {
            return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
        }
        if self.header.is_ack() {
            if self.header.payload_length != 0 || !buf.is_empty() {
                return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
            }
            let settings = Frame::new(
                self.header.stream_id,
                FrameFlags::new(self.header.flags),
                frame::Payload::Settings(Settings::new(vec![])),
            );

            return Ok(FrameKind::Complete(settings));
        }
        let mut settings = vec![];
        for chunk in buf.chunks(6) {
            if let Some(setting) = split_token_to_setting(chunk)? {
                settings.push(setting);
            }
        }
        let frame = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::Settings(Settings::new(settings)),
        );
        Ok(FrameKind::Complete(frame))
    }

    fn decode_data_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        if self.header.payload_length == 0 {
            let frame = Frame::new(
                self.header.stream_id,
                FrameFlags::new(self.header.flags),
                frame::Payload::Data(Data::new(vec![])),
            );
            return Ok(FrameKind::Complete(frame));
        }
        let is_padded = self.header.is_padded();
        let data = if is_padded {
            let padded_length = buf[0] as usize;
            if self.header.payload_length <= padded_length {
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
            }
            let data_end_index = self.header.payload_length - padded_length;
            let mut data: Vec<u8> = vec![];
            data.extend_from_slice(&buf[1..data_end_index]);
            data
        } else {
            let mut data: Vec<u8> = vec![];
            data.extend_from_slice(&buf[..self.header.payload_length]);
            data
        };
        let frame = Frame::new(
            self.header.stream_id,
            FrameFlags::new(self.header.flags),
            frame::Payload::Data(Data::new(data)),
        );
        Ok(FrameKind::Complete(frame))
    }

    fn decode_continuation_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        let end_headers = self.header.is_end_headers();
        if end_headers {
            self.hpack.hpack_decode(buf)?;
            let headers = self.hpack.hpack_finish()?;
            let frame = if self.continuations.promised_stream_id != 0 {
                Frame::new(
                    self.continuations.stream_id,
                    FrameFlags::new(self.continuations.flags),
                    frame::Payload::PushPromise(frame::PushPromise::new(
                        self.continuations.promised_stream_id,
                        headers,
                    )),
                )
            } else {
                Frame::new(
                    self.continuations.stream_id,
                    FrameFlags::new(self.continuations.flags),
                    frame::Payload::Headers(frame::Headers::new(headers)),
                )
            };
            self.continuations.reset();
            Ok(FrameKind::Complete(frame))
        } else {
            self.hpack.hpack_decode(buf)?;
            Ok(FrameKind::Partial)
        }
    }

    fn decode_headers_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        let priority = self.header.is_headers_priority();
        let is_padded = self.header.is_padded();
        let end_headers = self.header.is_end_headers();
        let end_stream = self.header.is_end_stream();

        let mut fragment_start_index = 0;
        let mut fragment_end_index = self.header.payload_length;
        if is_padded {
            let padded_length = buf[0] as usize;
            if self.header.payload_length <= padded_length {
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
            }
            fragment_start_index += 1;
            fragment_end_index -= padded_length;
        }
        if priority {
            fragment_start_index += 5;
        }

        if fragment_start_index > fragment_end_index {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        if end_headers {
            self.hpack
                .hpack_decode(&buf[fragment_start_index..fragment_end_index])?;
            let headers = self.hpack.hpack_finish()?;
            let frame = Frame::new(
                self.header.stream_id,
                FrameFlags::new(self.header.flags),
                frame::Payload::Headers(h2::frame::Headers::new(headers)),
            );
            Ok(FrameKind::Complete(frame))
        } else {
            self.continuations.flags = self.header.flags;
            self.continuations.is_end_stream = end_stream;
            self.continuations.is_end_headers = false;
            self.continuations.stream_id = self.header.stream_id;

            // TODO Performance optimization, The storage structure Vec is optimized. When a
            // complete field block exists in the buffer, fragments do not need to be copied
            // to the Vec.
            self.hpack
                .hpack_decode(&buf[fragment_start_index..fragment_end_index])?;
            Ok(FrameKind::Partial)
        }
    }

    fn decode_push_promise_payload(&mut self, buf: &[u8]) -> Result<FrameKind, H2Error> {
        if is_connection_frame(self.header.stream_id) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let buf = if self.offset != 0 {
            self.buffer.extend_from_slice(buf);
            self.offset += buf.len();
            self.buffer.as_slice()
        } else {
            buf
        };
        let is_padded = self.header.is_padded();
        let end_headers = self.header.is_end_headers();
        let mut fragment_start_index = 4;
        let mut fragment_end_index = self.header.payload_length;
        if is_padded {
            let padded_length = buf[0] as usize;
            if self.header.payload_length <= padded_length {
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
            }
            fragment_start_index += 1;
            fragment_end_index -= padded_length;
        }
        if fragment_start_index > fragment_end_index {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        let promised_stream_id = if is_padded {
            get_stream_id(&buf[1..5])
        } else {
            get_stream_id(&buf[..4])
        };
        if is_connection_frame(promised_stream_id as usize) {
            return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
        }
        if end_headers {
            self.hpack
                .hpack_decode(&buf[fragment_start_index..fragment_end_index])?;
            let headers = self.hpack.hpack_finish()?;
            let frame = Frame::new(
                self.header.stream_id,
                FrameFlags::new(self.header.flags),
                frame::Payload::PushPromise(h2::frame::PushPromise::new(
                    promised_stream_id,
                    headers,
                )),
            );
            Ok(FrameKind::Complete(frame))
        } else {
            self.continuations.flags = self.header.flags;
            self.continuations.is_end_headers = false;
            self.continuations.stream_id = self.header.stream_id;
            self.continuations.promised_stream_id = promised_stream_id;
            self.hpack
                .hpack_decode(&buf[fragment_start_index..fragment_end_index])?;
            Ok(FrameKind::Partial)
        }
    }

    fn decode_frame_header<'a>(&mut self, buf: &'a [u8]) -> Result<Option<&'a [u8]>, H2Error> {
        let payload_pos = FRAME_HEADER_LENGTH - self.offset;
        return if buf.len() < payload_pos {
            self.offset += buf.len();
            self.buffer.extend_from_slice(buf);
            Ok(None)
        } else {
            let header_buffer = if self.offset == 0 {
                buf
            } else {
                self.buffer.extend_from_slice(&buf[..payload_pos]);
                self.buffer.as_slice()
            };
            let payload_length = ((header_buffer[0] as usize) << 16)
                + ((header_buffer[1] as usize) << 8)
                + (header_buffer[2] as usize);
            if payload_length > self.max_frame_size as usize {
                return Err(H2Error::ConnectionError(ErrorCode::FrameSizeError));
            }
            let frame_type = header_buffer[3];
            let flags = header_buffer[4];
            let stream_id = get_stream_id(&header_buffer[5..9]) as usize;
            let frame_header = FrameHeader {
                stream_id,
                flags,
                frame_type,
                payload_length,
            };
            if self.offset != 0 {
                self.offset = 0;
                self.buffer.clear();
            }
            self.header = frame_header;
            Ok(Some(&buf[payload_pos..]))
        };
    }
}

fn is_connection_frame(id: usize) -> bool {
    id == 0
}

fn get_stream_id(token: &[u8]) -> u32 {
    (((token[0] & 0x7f) as u32) << 24)
        | ((token[1] as u32) << 16)
        | ((token[2] as u32) << 8)
        | (token[3] as u32)
}

fn get_code_value(token: &[u8]) -> u32 {
    ((token[0] as u32) << 24)
        | ((token[1] as u32) << 16)
        | ((token[2] as u32) << 8)
        | (token[3] as u32)
}

fn split_token_to_setting(token: &[u8]) -> Result<Option<Setting>, H2Error> {
    let id = u16::from(token[0]) << 8 | u16::from(token[1]);
    let value = get_code_value(&token[2..6]);
    get_setting(id, value)
}

pub fn get_setting(id: u16, value: u32) -> Result<Option<Setting>, H2Error> {
    match id {
        1 => Ok(Some(Setting::HeaderTableSize(value))),
        2 => {
            let enable_push = match value {
                0 => false,
                1 => true,
                _ => return Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
            };
            Ok(Some(Setting::EnablePush(enable_push)))
        }
        3 => Ok(Some(Setting::MaxConcurrentStreams(value))),
        4 => {
            if value as usize > MAX_INITIAL_WINDOW_SIZE {
                return Err(H2Error::ConnectionError(ErrorCode::FlowControlError));
            }
            Ok(Some(Setting::InitialWindowSize(value)))
        }
        5 => {
            if !(DEFAULT_MAX_FRAME_SIZE..=MAX_ALLOWED_MAX_FRAME_SIZE).contains(&value) {
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
            }
            Ok(Some(Setting::MaxFrameSize(value)))
        }
        6 => Ok(Some(Setting::MaxHeaderListSize(value))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod ut_frame_decoder {
    use crate::h2::decoder::{get_setting, FrameDecoder, FrameHeader, FrameKind};
    use crate::h2::frame::{Payload, Ping, Setting};
    use crate::h2::{ErrorCode, H2Error, PseudoHeaders};
    use crate::util::test_util::decode;

    macro_rules! check_complete_frame {
        (
            @header;
            FrameKind: $frame_kind:expr,
            Frame: {
                StreamId: $stream_id:expr,
                Flags: $flags:expr,
                Payload: $payload:expr,
                $(Padding: $frame_padding:expr,)?
            },
        ) => {
            match $frame_kind {
                FrameKind::Complete(frame) => {
                    assert_eq!(frame.stream_id(), $stream_id);
                    assert_eq!(frame.flags().bits(), $flags);
                    match frame.payload() {
                        Payload::Headers(headers_frame) => {
                            let (pseudo, header) = headers_frame.parts();
                            assert_eq!(
                                header.len(),
                                $payload.1.len(),
                                "assert header length failed"
                            );
                            for (key, value) in $payload.1.iter() {
                                assert_eq!(header.get(*key).unwrap().to_string().unwrap(), *value);
                            }
                            for (key, value) in $payload.0.iter() {
                                match *key {
                                    ":method" => {
                                        assert_eq!(
                                            pseudo.method().expect("pseudo.method get failed !"),
                                            *value
                                        );
                                    }
                                    ":scheme" => {
                                        assert_eq!(
                                            pseudo.scheme().expect("pseudo.scheme get failed !"),
                                            *value
                                        );
                                    }
                                    ":authority" => {
                                        assert_eq!(
                                            pseudo
                                                .authority()
                                                .expect("pseudo.authority get failed !"),
                                            *value
                                        );
                                    }
                                    ":path" => {
                                        assert_eq!(
                                            pseudo.path().expect("pseudo.path get failed !"),
                                            *value
                                        );
                                    }
                                    ":status" => {
                                        assert_eq!(
                                            pseudo.status().expect("pseudo.status get failed !"),
                                            *value
                                        );
                                    }
                                    _ => {
                                        panic!("Unexpected pseudo header input !");
                                    }
                                }
                            }
                        }
                        _ => {
                            panic!("Unrecognized frame type !");
                        }
                    }
                }
                FrameKind::Partial => {
                    panic!("Incorrect decode result !");
                }
            };
        };
        (
            @data;
            FrameKind: $frame_kind:expr,
            Frame: {
                StreamId: $stream_id:expr,
                Flags: $flags:expr,
                Payload: $payload:expr,
                $(Padding: $frame_padding:expr,)?
            },
        ) => {
            match $frame_kind {
                FrameKind::Complete(frame) => {
                    assert_eq!(frame.stream_id(), $stream_id);
                    assert_eq!(frame.flags().bits(), $flags);
                    match frame.payload() {
                        Payload::Data(data) => {
                            assert_eq!(data.data().as_slice(), $payload.as_bytes())
                        }
                        _ => {
                            panic!("Unrecognized frame type !");
                        }
                    }
                }
                FrameKind::Partial => {
                    panic!("Incorrect decode result !");
                }
            };
        };
        (
            @partial;
            FrameKind: $frame_kind:expr,
        ) => {
            match $frame_kind {
                FrameKind::Complete(_) => {
                    panic!("Incorrect decode result !");
                }
                FrameKind::Partial => {}
            }
        };
    }

    macro_rules! decode_frames {
        (
            @data;
            Bytes: $frame_hex:expr,
            Count: $frame_count:expr,
            $(
                Frame: {
                    StreamId: $stream_id:expr,
                    Flags: $flags:expr,
                    Payload: $payload:expr,
                    $(Padding: $frame_padding:expr,)?
                },
            )*

        ) => {
            let mut decoder = FrameDecoder::default();
            let frame_bytes = decode($frame_hex).expect("convert frame hex to bytes failed !");
            let decoded_frames = decoder.decode(frame_bytes.as_slice()).expect("decode frame bytes failed !");
            assert_eq!(decoded_frames.len(), $frame_count);
            let mut frames_iter = decoded_frames.iter();
            $(
            check_complete_frame!(
                @data;
                FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
                Frame: {
                    StreamId: $stream_id,
                    Flags: $flags,
                    Payload: $payload,
                },
            );
            )*

        };
        (
            @header;
            Bytes: $frame_hex:expr,
            Count: $frame_count:expr,
            $(
                Frame: {
                    StreamId: $stream_id:expr,
                    Flags: $flags:expr,
                    Payload: $payload:expr,
                    $(Padding: $frame_padding:expr,)?
                },
            )*
        ) => {
            let mut decoder = FrameDecoder::default();
            let frame_bytes = decode($frame_hex).expect("convert frame hex to bytes failed !");
            let decoded_frames = decoder.decode(frame_bytes.as_slice()).expect("decode frame bytes failed !");
            assert_eq!(decoded_frames.len(), $frame_count);
            let mut frames_iter = decoded_frames.iter();
            $(
            check_complete_frame!(
                @header;
                FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
                Frame: {
                    StreamId: $stream_id,
                    Flags: $flags,
                    Payload: $payload,
                },
            );
            )*
        };
        (
            @error;
            Bytes: $frame_hex:expr,
            Decoder: {
                MAX_HEADER_LIST_SIZE: $header_list_size: expr,
                MAX_FRAME_SIZE: $max_frame_size: expr,
            },
            Error: $error_type:expr,
        ) => {
            let mut decoder = FrameDecoder::new();
            decoder.set_max_header_list_size($header_list_size);
            decoder.set_max_frame_size($max_frame_size).expect("Illegal size of SETTINGS_MAX_FRAME_SIZE !");
            let frame_bytes = decode($frame_hex).expect("convert frame hex to bytes failed !");
            let decoded_frames = decoder.decode(frame_bytes.as_slice());
            assert!(decoded_frames.is_err());
            assert_eq!(decoded_frames.err().expect("Get Error type failed !"), $error_type);
        };
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a simple complete DATA frame.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_complete_data_frame() {
        decode_frames!(
            @data;
            Bytes: "00000b00010000000168656c6c6f20776f726c64",
            Count: 1,
            Frame: {
                StreamId: 1,
                Flags: 1,
                Payload: "hello world",
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a complete padded DATA frame with padding.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_complete_padded_data_frame() {
        decode_frames!(
            @data;
            Bytes: "0000140008000000020648656C6C6F2C20776F726C6421486F77647921",
            Count: 1,
            Frame: {
                StreamId: 2,
                Flags: 8,
                Payload: "Hello, world!",
                Padding: "Howdy!",
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test Data Frames in Segments.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_segmented_data_frame() {
        // (stream_id, flags, is_end_stream,  content)
        let mut decoder = FrameDecoder::default();
        let frame_bytes = decode("00000b00010000000168656c6c6f20776f726c640000140008000000020648656C6C6F2C20776F726C6421486F77647921").unwrap();
        let frame_bytes = frame_bytes.as_slice();
        let decoded_frames = decoder.decode(&frame_bytes[..8]).unwrap();
        assert_eq!(decoded_frames.len(), 0);
        let decoded_frames = decoder.decode(&frame_bytes[8..12]).unwrap();
        assert_eq!(decoded_frames.len(), 0);
        let decoded_frames = decoder.decode(&frame_bytes[12..24]).unwrap();
        assert_eq!(decoded_frames.len(), 1);
        let mut frames_iter = decoded_frames.iter();
        check_complete_frame!(
            @data;
            FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
            Frame: {
                StreamId: 1,
                Flags: 1,
                Payload: "hello world",
            },
        );
        let decoded_frames = decoder.decode(&frame_bytes[24..]).unwrap();
        let mut frames_iter = decoded_frames.iter();
        check_complete_frame!(
            @data;
            FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
            Frame: {
                StreamId: 2,
                Flags: 8,
                Payload: "Hello, world!",
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a complete Request HEADERS Frames with padding and priority.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_complete_padded_priority_headers_frame() {
        decode_frames!(
            @header;
            Bytes: "000040012D000000011080000014098286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f546869732069732070616464696E672E",
            Count: 1,
            Frame: {
                StreamId: 1,
                Flags: 45,
                Payload:(
                    [(":method", "GET"), (":scheme", "http"), (":authority", "127.0.0.1:3000"), (":path", "/resource")],
                    [("host", "127.0.0.1"), ("accept", "image/jpeg")]),
                Padding: "This is padding.",
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a complete Response HEADERS Frames.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_complete_response_headers_frame() {
        decode_frames!(
            @header;
            Bytes: "0000390105000000018b0f1385f3ebdfbf5f6496dc34fd2826d4d03b141004ca8015c0b9702053168dff6196dc34fd2826d4d03b141004ca806ee361b82654c5a37f",
            Count: 1,
            Frame: {
                StreamId: 1,
                Flags: 5,
                Payload:(
                    [(":status", "304")],
                    [("etag", "xyzzy"), ("expires", "Sat, 25 Mar 2023 02:16:10 GMT"), ("date", "Sat, 25 Mar 2023 05:51:23 GMT")]),
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test HEADERS Frames exceeded by SETTINGS_MAX_HEADER_LIST_SIZE.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_size_exceeded_headers_frame() {
        decode_frames!(
            @error;
            Bytes: "00002a0105000000018286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f",
            Decoder: {
                MAX_HEADER_LIST_SIZE: 60,
                MAX_FRAME_SIZE: 2 << 13,
            },
            Error: H2Error::ConnectionError(ErrorCode::ConnectError),
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a series of complete request Frames.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    /// Test a complete `Request` frames.
    #[test]
    fn ut_frame_decoder_with_series_request_frames() {
        let mut decoder = FrameDecoder::default();
        let frame_bytes = decode("00002e0100000000018286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f0f0d817f0000040904000000010f0d817f0000090001000000017468697320626f6479").unwrap();
        let decoded_frames = decoder.decode(frame_bytes.as_slice()).unwrap();
        assert_eq!(decoded_frames.len(), 3);
        let mut frames_iter = decoded_frames.iter();

        // HEADERS frame END_HEADERS Flag is false, so  it returns Partial
        check_complete_frame!(
            @partial;
            FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
        );

        // continuation is ("content-length", "9"), so it will append to headers'
        // content-length because of repeat.
        check_complete_frame!(
            @header;
            FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
            Frame: {
                StreamId: 1,
                Flags: 0,
                Payload: (
                    [(":method", "GET"), (":scheme", "http"), (":authority", "127.0.0.1:3000"), (":path", "/resource"),],
                    [("host", "127.0.0.1"), ("accept", "image/jpeg"), ("content-length", "9, 9")]),
            },
        );
        check_complete_frame!(
            @data;
            FrameKind: frames_iter.next().expect("take next frame from iterator failed !"),
            Frame: {
                StreamId: 1,
                Flags: 1,
                Payload: "this body",
            },
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test the function of inserting HEADERS of other streams between HEADERS
    /// and CONTINUATION of the same stream.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_headers_frame_in_another_stream() {
        decode_frames!(
            @error;
            Bytes: "00002e0100000000018286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f0f0d817f00002e0104000000028286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f0f0d817f0000040904000000010f0d817f0000090001000000017468697320626f6479",
            Decoder: {
                MAX_HEADER_LIST_SIZE: 16 << 20,
                MAX_FRAME_SIZE: 2 << 13,
            },
            Error: H2Error::ConnectionError(ErrorCode::ProtocolError),
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test the function of inserting CONTINUATION of other streams between
    /// HEADERS and CONTINUATION of the same stream.
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_continuation_frame_in_another_stream() {
        decode_frames!(
            @error;
            Bytes: "00002e0100000000018286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f0f0d817f0000040904000000020f0d817f0000090001000000017468697320626f6479",
            Decoder: {
                MAX_HEADER_LIST_SIZE: 16 << 20,
                MAX_FRAME_SIZE: 2 << 13,
            },
            Error: H2Error::ConnectionError(ErrorCode::ProtocolError),
        );
    }

    /// UT test cases for `FrameDecoder::decode`.
    ///
    /// # Brief
    ///
    /// Test a complete Request HEADERS Frames with padding and priority, the
    /// purpose is to test the method of `FrameFlags`.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode` method.
    /// 3. Checks the results.
    #[test]
    fn ut_frame_decoder_with_padded_end_stream_headers_frame() {
        let mut decoder = FrameDecoder::default();
        let frame_bytes = decode("000040012D000000011080000014098286418a089d5c0b8170dc640007048762c2a0f6d842ff6687089d5c0b8170ff5388352398ac74acb37f546869732069732070616464696E672E").unwrap();
        let decoded_frames = decoder.decode(frame_bytes.as_slice()).unwrap();
        let frames_kind = decoded_frames.iter().next().unwrap();
        match frames_kind {
            FrameKind::Complete(frame) => {
                assert!(frame.flags().is_padded());
                assert!(frame.flags().is_end_stream());
                assert_eq!(frame.flags().bits(), 0x2D);
            }
            FrameKind::Partial => {
                panic!("Unexpected FrameKind !")
            }
        }
    }

    /// UT test cases for `FrameDecoder::decode_ping_payload`.
    ///
    /// # Brief
    ///
    /// Tests the case of a ping payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode_ping_payload` method.
    /// 3. Checks the results.
    #[test]
    fn ut_decode_ping_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 8,
            frame_type: 0x6,
            flags: 0x0,
            stream_id: 0x0,
        };
        let ping_payload = &[b'p', b'i', b'n', b'g', b't', b'e', b's', b't'];
        let frame_kind = decoder.decode_ping_payload(ping_payload).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::Ping(ping) => {
                    let data = ping.data();
                    assert_eq!(data.len(), 8);
                    assert_eq!(data[0], 112);
                    assert_eq!(data[7], 116);
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the payload length is not 8, which should return an
        // error.
        decoder.header.payload_length = 7;
        let result = decoder.decode_ping_payload(ping_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::FrameSizeError))
        ));

        // Tests the case where the stream id is not 0, which should return an error.
        decoder.header.stream_id = 1;
        let result = decoder.decode_ping_payload(ping_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_priority_payload`.
    ///
    /// # Brief
    ///
    /// This test case checks the behavior of the `decode_priority_payload`
    /// method in two scenarios.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `decode_priority_payload` method with a valid
    ///    `priority_payload`.
    /// 3. Verifies the method correctly decodes the payload and returns the
    ///    expected values.
    /// 4. Sets the `stream_id` in the header to 0 and checks if the method
    ///    returns an error as expected.
    #[test]
    fn ut_decode_priority_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 5,
            frame_type: 0x2,
            flags: 0x0,
            stream_id: 0x1,
        };
        let priority_payload = &[0x80, 0x0, 0x0, 0x1, 0x20];
        let frame_kind = decoder.decode_priority_payload(priority_payload).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::Priority(priority) => {
                    assert!(priority.get_exclusive());
                    assert_eq!(priority.get_stream_dependency(), 1);
                    assert_eq!(priority.get_weight(), 32);
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the stream id is 0, which should return an error.
        decoder.header.stream_id = 0;
        let result = decoder.decode_priority_payload(priority_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_goaway_payload`.
    ///
    /// # Brief
    ///
    /// This test case checks the behavior of the `decode_goaway_payload` method
    /// in two scenarios.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `decode_goaway_payload` method with a valid
    ///    `goaway_payload`.
    /// 3. Verifies the method correctly decodes the payload and returns the
    ///    expected values.
    /// 4. Sets the `stream_id` in the header to a non-zero value and checks if
    ///    the method returns an error as expected.
    #[test]
    fn ut_decode_goaway_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 13,
            frame_type: 0x7,
            flags: 0x0,
            stream_id: 0x0,
        };
        let goaway_payload = &[
            0x0, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0x2, b'd', b'e', b'b', b'u', b'g',
        ];
        let frame_kind = decoder.decode_goaway_payload(goaway_payload).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::Goaway(goaway) => {
                    assert_eq!(goaway.get_last_stream_id(), 1);
                    assert_eq!(goaway.get_error_code(), 2);
                    assert_eq!(goaway.get_debug_data(), b"debug");
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the stream id is not 0, which should return an error.
        decoder.header.stream_id = 1;
        let result = decoder.decode_goaway_payload(goaway_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_window_update_payload`.
    ///
    /// # Brief
    ///
    /// Tests the case of a window update payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode_window_update_payload` method.
    /// 3. Checks the results.
    #[test]
    fn ut_decode_window_update_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 4,
            frame_type: 0x8,
            flags: 0x0,
            stream_id: 0x1,
        };
        let window_update_payload = &[0x0, 0x0, 0x0, 0x1];
        let frame_kind = decoder
            .decode_window_update_payload(window_update_payload)
            .unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::WindowUpdate(_) => {
                    // println!("{:?}", window_update.get_increment());
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the payload length is not 4, which should return an
        // error.
        decoder.header.payload_length = 5;
        let result = decoder.decode_window_update_payload(window_update_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::FrameSizeError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_reset_payload`.
    ///
    /// # Brief
    ///
    /// Tests the case of a reset payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode_reset_payload` method.
    /// 3. Checks the results.
    #[test]
    fn ut_decode_reset_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 4,
            frame_type: 0x3,
            flags: 0x0,
            stream_id: 0x1,
        };
        let reset_payload = &[0x0, 0x0, 0x0, 0x1];
        let frame_kind = decoder.decode_reset_payload(reset_payload).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::RstStream(reset_stream) => {
                    assert_eq!(reset_stream.error_code(), 1);
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the payload length is not 4, which should return an
        // error.
        decoder.header.payload_length = 5;
        let result = decoder.decode_reset_payload(reset_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::FrameSizeError))
        ));

        // Tests the case where the stream id is 0, which should return an error.
        decoder.header.stream_id = 0;
        let result = decoder.decode_reset_payload(reset_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_settings_payload`.
    ///
    /// # Brief
    ///
    /// Tests the case of a settings payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode_settings_payload` method.
    /// 3. Checks the results.
    #[test]
    fn ut_decode_settings_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 12,
            frame_type: 0x4,
            flags: 0x0,
            stream_id: 0x0,
        };

        // Mock a settings payload: [0x00, 0x01, 0x00, 0x00, 0x00, 0x80, 0x00, 0x02,
        // 0x00, 0x00, 0x00, 0x01] Setting 1: Header Table Size, Value: 128
        // Setting 2: Enable Push, Value: 1
        let settings_payload = &[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x80, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01,
        ];
        let frame_kind = decoder.decode_settings_payload(settings_payload).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::Settings(settings) => {
                    let settings_vec = settings.get_settings();
                    assert_eq!(settings_vec.len(), 2);
                    assert_eq!(settings_vec[0], Setting::HeaderTableSize(128));
                    assert_eq!(settings_vec[1], Setting::EnablePush(true));
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Test the case where the settings frame is an acknowledgment.
        // For this, we should set the ACK flag (0x1) and the payload length should be
        // 0.
        decoder.header = FrameHeader {
            payload_length: 0,
            frame_type: 0x4,
            flags: 0x1,
            stream_id: 0x0,
        };
        let frame_kind = decoder.decode_settings_payload(&[]).unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::Settings(settings) => {
                    let settings_vec = settings.get_settings();
                    assert_eq!(settings_vec.len(), 0);
                }
                _ => panic!("Unexpected payload type!"),
            },
            FrameKind::Partial => {
                panic!("Unexpected FrameKind!")
            }
        }

        // Tests the case where the payload length is not a multiple of 6, which should
        // return an error.
        decoder.header.payload_length = 5;
        let result = decoder.decode_settings_payload(settings_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::FrameSizeError))
        ));

        // Tests the case where the stream id is not 0, which should return an error.
        decoder.header.stream_id = 1;
        let result = decoder.decode_settings_payload(settings_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::decode_push_promise_payload`.
    ///
    /// # Brief
    ///
    /// Tests the case of a push promise payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::decode_push_promise_payload` method.
    /// 3. Checks the results.
    #[test]
    fn ut_decode_push_promise_payload() {
        let mut decoder = FrameDecoder::new();
        decoder.header = FrameHeader {
            payload_length: 10,
            frame_type: 0x5,
            flags: 0x88,
            stream_id: 0x1,
        };
        let push_promise_payload = &[0x0, 0x0, 0x0, 0x2, b'h', b'e', b'l', b'l', b'o', b'w'];

        // Tests the case where the payload is a valid push promise.
        let frame_kind = decoder
            .decode_push_promise_payload(push_promise_payload)
            .unwrap();
        match frame_kind {
            FrameKind::Complete(frame) => match frame.payload() {
                Payload::PushPromise(_) => {}
                _ => panic!("Unexpected payload type!"),
            },

            FrameKind::Partial => {}
        }

        // Tests the case where the payload length is less than the promised_stream_id
        // size, which should return an error.
        decoder.header.payload_length = 3;
        let result = decoder.decode_push_promise_payload(push_promise_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));

        // Tests the case where the stream id is 0, which should return an error.
        decoder.header.stream_id = 0;
        let result = decoder.decode_push_promise_payload(push_promise_payload);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));
    }

    /// UT test cases for `FrameDecoder::get_setting`.
    ///
    /// # Brief
    ///
    /// Tests the case of a settings payload.
    ///
    /// 1. Creates a `FrameDecoder`.
    /// 2. Calls its `FrameDecoder::get_setting` method.
    /// 3. Checks the results.
    #[test]
    fn ut_get_setting() {
        // Test the case where the id is for a HeaderTableSize
        match get_setting(1, 4096).unwrap() {
            Some(Setting::HeaderTableSize(size)) => {
                assert_eq!(size, 4096);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for an EnablePush
        match get_setting(2, 0).unwrap() {
            Some(Setting::EnablePush(push)) => {
                assert!(!push);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for a MaxConcurrentStreams
        match get_setting(3, 100).unwrap() {
            Some(Setting::MaxConcurrentStreams(streams)) => {
                assert_eq!(streams, 100);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for an InitialWindowSize
        match get_setting(4, 20000).unwrap() {
            Some(Setting::InitialWindowSize(size)) => {
                assert_eq!(size, 20000);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for a MaxFrameSize
        match get_setting(5, 16384).unwrap() {
            Some(Setting::MaxFrameSize(size)) => {
                assert_eq!(size, 16384);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for a MaxHeaderListSize
        match get_setting(6, 8192).unwrap() {
            Some(Setting::MaxHeaderListSize(size)) => {
                assert_eq!(size, 8192);
            }
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is not recognized
        match get_setting(7, 1000).unwrap() {
            None => {}
            _ => panic!("Unexpected Setting!"),
        };

        // Test the case where the id is for an EnablePush, but the value is invalid
        let result = get_setting(2, 2);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::ProtocolError))
        ));

        // Test the case where the id is for an InitialWindowSize, but the value is too
        // large
        let result = get_setting(4, 2usize.pow(31) as u32);
        assert!(matches!(
            result,
            Err(H2Error::ConnectionError(ErrorCode::FlowControlError))
        ));
    }
}
