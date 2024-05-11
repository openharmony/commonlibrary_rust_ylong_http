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

use octets::{Octets, OctetsMut};

use crate::h3::frame_new::{Headers, Payload};
// use crate::h3::octets::WriteVarint;
use crate::h3::qpack::table::DynamicTable;
use crate::h3::qpack::{DecoderInst, QpackEncoder};
use crate::h3::{frame_new, Frame};

#[derive(PartialEq, Debug)]
enum FrameEncoderState {
    // The initial state for the frame encoder.
    Idle,
    FrameComplete,
    PayloadComplete,
    // Header Frame
    EncodingHeadersFrame,
    EncodingHeadersPayload,
    // Data Frame
    EncodingDataFrame,
    EncodingDataPaylaod,
    // CancelPush Frame
    EncodingCancelPushFrame,
    EncodingCancelPushPayload,
    // Settings Frame
    EncodingSettingsFrame,
    EncodingSettingsPayload,
    // PushPromise Frame
    EncodingPushPromiseFrame,
    EncodingPushPromisePayload,
    // Goaway Frame
    EncodingGoawayFrame,
    EncodingGoawayPayload,
    // MaxPushId Frame
    EncodingMaxPushIdFrame,
    EncodingMaxPushIdPayload,
}

pub struct FrameEncoder<'a> {
    qpack_encoder: QpackEncoder<'a>,
    stream_id: usize,
    // other frames
    current_frame: Option<Frame>,
    state: FrameEncoderState,
    encoded_bytes: usize,
    buf_offset: usize,
    payload_offset: usize,
}

impl<'a> FrameEncoder<'a> {
    /// Create a FrameEncoder
    /// note: user should give the qpack's dynamic table, which is shared with
    /// Decoder.
    pub(crate) fn new(
        table: &'a mut DynamicTable,
        qpack_all_post: bool,
        qpack_drain_index: usize,
        stream_id: usize,
    ) -> Self {
        Self {
            qpack_encoder: QpackEncoder::new(table, stream_id, qpack_all_post, qpack_drain_index),
            stream_id,
            current_frame: None,
            state: FrameEncoderState::Idle,
            encoded_bytes: 0,
            buf_offset: 0,
            payload_offset: 0,
        }
    }

    /// Sets the current frame to be encoded by the `FrameEncoder`. The state of
    /// the encoder is updated based on the payload type of the frame.
    pub fn set_frame(&mut self, frame: Frame) {
        self.current_frame = Some(frame);
        // Reset the encoded bytes counter
        self.encoded_bytes = 0;
        // set frame state
        match &self.current_frame {
            Some(frame) => match frame.frame_type() {
                &frame_new::HEADERS_FRAME_TYPE_ID => {
                    if let Payload::Headers(h) = frame.payload() {
                        // todo! header压缩
                        self.qpack_encoder.set_parts(h.get_part());
                        // complete output in one go.
                        let payload_size =
                            self.qpack_encoder.encode(&mut self.header_payload_buffer);
                        self.remaining_header_payload = payload_size;
                        self.state = FrameEncoderState::EncodingHeadersFrame;
                    }
                }
                &frame_new::DATA_FRAME_TYPE_ID => self.state = FrameEncoderState::EncodingDataFrame,
                &frame_new::CANCEL_PUSH_FRAME_TYPE_ID => {
                    self.state = FrameEncoderState::EncodingCancelPushFrame
                }
                &frame_new::SETTINGS_FRAME_TYPE_ID => {
                    self.state = FrameEncoderState::EncodingSettingsFrame
                }
                &frame_new::PUSH_PROMISE_FRAME_TYPE_ID => {
                    self.state = FrameEncoderState::EncodingPushPromiseFrame
                }
                &frame_new::GOAWAY_FRAME_TYPE_ID => {
                    self.state = FrameEncoderState::EncodingGoawayFrame
                }
                &frame_new::MAX_PUSH_FRAME_TYPE_ID => {
                    self.state = FrameEncoderState::EncodingMaxPushIdFrame
                }
                _ => {}
            },
            None => self.state = FrameEncoderState::Idle,
        }
    }

    fn encode_payload(&self, buf: &mut OctetsMut, data: &[u8], start: usize) -> usize {
        let data_len = data.len();
        let remaining_data_bytes = data_len.saturating_sub(start);
        let bytes_to_write = remaining_data_bytes.min(buf.len());
        // use unwrap, because data len must be smaller than buf len
        buf.put_bytes(&data[start..start + bytes_to_write]).unwrap();
        bytes_to_write
    }

    fn encode_frame(&self, frame_ref: Option<&Frame>, buf: &mut [u8]) -> Result<usize, Err> {
        if let Some(frame) = frame_ref {
            let mut octet_buf = OctetsMut::with_slice(buf);
            octet_buf.put_varint(frame.frame_type().clone())?;
            octet_buf.put_varint(frame.frame_len().clone())?;
            let size = octet_buf.off();
            Ok(size)
        } else {
            Err(FrameEncoderErr::NoCurrentFrame)
        }
    }

    pub fn encode(&mut self, buf: &mut [u8]) -> Result<usize, Err> {
        let mut written_bytes = 0;

        while written_bytes < buf.len() {
            match self.state {
                FrameEncoderState::Idle
                | FrameEncoderState::PayloadComplete
                | FrameEncoderState::FrameComplete => {
                    break;
                }
                FrameEncoderState::EncodingHeadersFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingHeadersPayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingHeadersPayload => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::Headers(h) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let size_remain = buf_remain.len();
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            if (h.get_headers().len() - self.payload_offset) < size_remain {
                                self.encode_payload(
                                    &mut octet_buf,
                                    h.get_headers(),
                                    self.payload_offset,
                                );
                                self.payload_offset = 0;
                                self.state = FrameEncoderState::PayloadComplete;
                            } else {
                                let writen_bytes = self.encode_payload(
                                    &mut octet_buf,
                                    h.get_headers(),
                                    self.payload_offset,
                                );
                                self.payload_offset += writen_bytes;
                                self.encoded_bytes += writen_bytes;
                            }
                            let size = octet_buf.off();
                            Ok(size)
                        } else {
                            Err(FrameEncoderErr)
                        }
                    } else {
                        Err(FrameEncoderErr::NoCurrentFrame)
                    }
                }

                FrameEncoderState::EncodingDataFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingDataPaylaod;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingDataPaylaod => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::Data(d) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let size_remain = buf_remain.len();
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            if (d.data().len() - self.payload_offset) < size_remain {
                                self.encode_payload(&mut octet_buf, d.data(), self.payload_offset);
                                self.payload_offset = 0;
                                self.state = FrameEncoderState::PayloadComplete;
                            } else {
                                let writen_bytes = self.encode_payload(
                                    &mut octet_buf,
                                    d.data(),
                                    self.payload_offset,
                                );
                                self.payload_offset += writen_bytes;
                                self.encoded_bytes += writen_bytes;
                            }
                            let size = octet_buf.off();
                            Ok(size)
                        } else {
                            Err(FrameEncoderErr)
                        }
                    } else {
                        Err(FrameEncoderErr::NoCurrentFrame)
                    }
                }

                FrameEncoderState::EncodingCancelPushFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingCancelPushPayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingCancelPushPayload => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::CancelPush(cp) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            octet_buf.put_varint(cp.get_push_id().clone())?;
                            self.state = FrameEncoderState::PayloadComplete;
                            let size = octet_buf.off();
                            Ok(size)
                        } else {
                            Err(FrameEncoderErr)
                        }
                    } else {
                        Err(FrameEncoderErr::NoCurrentFrame)
                    }
                }

                FrameEncoderState::EncodingSettingsFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingSettingsPayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingSettingsPayload => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::Settings(s) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            if let Some(val) = s.get_max_fied_section_size() {
                                octet_buf.put_varint(frame_new::SETTINGS_MAX_FIELD_SECTION_SIZE)?;
                                octet_buf.put_varint(val.clone())?;
                            }

                            if let Some(val) = s.get_qpack_max_table_capacity() {
                                octet_buf
                                    .put_varint(frame_new::SETTINGS_QPACK_MAX_TABLE_CAPACITY)?;
                                octet_buf.put_varint(val.clone())?;
                            }

                            if let Some(val) = s.get_qpack_block_stream() {
                                octet_buf.put_varint(frame_new::SETTINGS_QPACK_BLOCKED_STREAMS)?;
                                octet_buf.put_varint(val.clone())?;
                            }

                            if let Some(val) = s.get_connect_protocol_enabled() {
                                octet_buf
                                    .put_varint(frame_new::SETTINGS_ENABLE_CONNECT_PROTOCOL)?;
                                octet_buf.put_varint(val.clone())?;
                            }

                            if let Some(val) = s.get_h3_datagram() {
                                octet_buf.put_varint(frame_new::SETTINGS_H3_DATAGRAM_00)?;
                                octet_buf.put_varint(val.clone())?;
                                octet_buf.put_varint(frame_new::SETTINGS_H3_DATAGRAM)?;
                                octet_buf.put_varint(val.clone())?;
                            }

                            if octet_buf.off() == 0 {
                                Err(FrameEncoderErr::NoCurrentFrame)
                            }
                            self.encoded_bytes += octet_buf.off();
                            self.state = FrameEncoderState::PayloadComplete;
                            Ok(octet_buf.off())
                        }
                    }
                }

                FrameEncoderState::EncodingPushPromiseFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingPushPromisePayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                // todo!
                FrameEncoderState::EncodingPushPromisePayload => {}

                FrameEncoderState::EncodingGoawayFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingGoawayPayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingGoawayPayload => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::Goaway(g) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            octet_buf.put_varint(g.get_id().clone())?;
                            self.state = FrameEncoderState::PayloadComplete;
                            let size = octet_buf.off();
                            Ok(size)
                        } else {
                            Err(FrameEncoderErr)
                        }
                    } else {
                        Err(FrameEncoderErr::NoCurrentFrame)
                    }
                }

                FrameEncoderState::EncodingMaxPushIdFrame => {
                    match self.encode_frame(self.current_frame.as_ref(), buf) {
                        Ok(size) => {
                            self.encoded_bytes += size;
                            self.state = FrameEncoderState::EncodingMaxPushIdPayload;
                        }
                        Err(_) => Err(FrameEncoderErr::NoCurrentFrame),
                    }
                }
                FrameEncoderState::EncodingMaxPushIdPayload => {
                    if let Some(frame) = self.current_frame.as_ref() {
                        if let Payload::MaxPushId(max) = frame.payload() {
                            let buf_remain = &mut buf[self.encoded_bytes..];
                            let mut octet_buf = OctetsMut::with_slice(buf_remain);
                            octet_buf.put_varint(max.get_id().clone())?;
                            self.state = FrameEncoderState::PayloadComplete;
                            let size = octet_buf.off();
                            Ok(size)
                        } else {
                            Err(FrameEncoderErr)
                        }
                    } else {
                        Err(FrameEncoderErr::NoCurrentFrame)
                    }
                }
                _ => {}
            }
        }
        Ok(written_bytes)
    }

    /// Encoder can modify size of the dynamic table, initial size of the table
    /// is 0. the size can also be updated from decoder
    pub(crate) fn update_dyn_size(&mut self, max_size: usize) {
        let cur_qpack = self.qpack_encoder.set_capacity(
            max_size,
            &mut self.qpack_encoder_buffer[self.remaining_qpack_payload..],
        );
        self.remaining_qpack_payload += cur_qpack;
    }

    /// User call `encode_header` to encode a header.
    pub fn encode_header(&mut self, headers: &Headers) {
        self.qpack_encoder.set_parts(headers.get_parts());
        let (cur_qpack, cur_header, _) = self.qpack_encoder.encode(
            &mut self.qpack_encoder_buffer[self.remaining_qpack_payload..],
            &mut self.header_payload_buffer[self.remaining_header_payload..],
        );
        self.remaining_header_payload += cur_header;
        self.remaining_qpack_payload += cur_qpack;
    }

    /// User must call `finish_encode_header` to end a batch of `encode_header`,
    /// so as to add prefix to this stream.
    pub fn finish_encode_header(&mut self) {
        let (cur_qpack, cur_header, mut prefix) = self.qpack_encoder.encode(
            &mut self.qpack_encoder_buffer[self.remaining_qpack_payload..],
            &mut self.header_payload_buffer[self.remaining_header_payload..],
        );
        self.remaining_header_payload += cur_header;
        self.remaining_qpack_payload += cur_qpack;
        if let Some((prefix_buf, cur_prefix)) = prefix {
            self.header_payload_buffer
                .copy_within(0..self.remaining_header_payload, cur_prefix);
            self.header_payload_buffer[..cur_prefix].copy_from_slice(&prefix_buf[..cur_prefix]);
            self.remaining_header_payload += cur_prefix;
        }
    }

    /// User call `decode_ins` to decode peer's qpack_decoder_stream.
    pub fn decode_ins(&mut self, buf: &[u8]) {
        match self.qpack_encoder.decode_ins(buf) {
            Ok(Some(DecoderInst::StreamCancel)) => {
                // todo: cancel this stream.
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod ut_headers_encode {
    use crate::h3::encoder::FrameEncoder;
    use crate::h3::frame::Headers;
    use crate::h3::parts::Parts;
    use crate::h3::qpack::table::{DynamicTable, Field};
    use crate::test_util::decode;

    /// `s_res`: header stream after encoding by QPACK.
    /// `q_res`: QPACK stream after encoding by QPACK.
    #[test]
    /// The encoder sends an encoded field section containing a literal
    /// representation of a field with a static name reference.
    fn literal_field_line_with_name_reference() {
        let mut table = DynamicTable::with_empty();
        let mut f_encoder = FrameEncoder::new(&mut table, false, 0, 0);

        let s_res = decode("0000510b2f696e6465782e68746d6c").unwrap();
        let headers = [(Field::Path, String::from("/index.html"))];

        for (field, value) in headers.iter() {
            let mut part = Parts::new();
            println!("encoding: HEADER: {:?} , VALUE: {:?}", field, value);
            part.update(field.clone(), value.clone());
            let header = Headers::new(part.clone());
            f_encoder.encode_header(&header);
        }
        f_encoder.finish_encode_header();
        println!(
            "header_payload_buffer: {:?}",
            f_encoder.header_payload_buffer[..f_encoder.remaining_header_payload].to_vec()
        );
        assert_eq!(
            s_res,
            f_encoder.header_payload_buffer[..f_encoder.remaining_header_payload].to_vec()
        );
    }

    #[test]
    /// The encoder sets the dynamic table capacity, inserts a header with a
    /// dynamic name reference, then sends a potentially blocking, encoded
    /// field section referencing this new entry. The decoder acknowledges
    /// processing the encoded field section, which implicitly acknowledges
    /// all dynamic table insertions up to the Required Insert Count.
    fn dynamic_table() {
        let mut table = DynamicTable::with_empty();

        let mut f_encoder = FrameEncoder::new(&mut table, true, 0, 0);

        f_encoder.update_dyn_size(220);
        let s_res = decode("03811011").unwrap();
        let q_res =
            decode("3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468").unwrap();
        let headers = [
            (Field::Authority, String::from("www.example.com")),
            (Field::Path, String::from("/sample/path")),
        ];
        for (field, value) in headers.iter() {
            println!("encoding: HEADER: {:?} , VALUE: {:?}", field, value);
            let mut part = Parts::new();
            part.update(field.clone(), value.clone());
            let header = Headers::new(part.clone());
            f_encoder.encode_header(&header);
        }
        f_encoder.finish_encode_header();
        println!(
            "header_payload_buffer: {:?}",
            f_encoder.header_payload_buffer[..f_encoder.remaining_header_payload].to_vec()
        );
        println!(
            "qpack_encoder_buffer: {:?}",
            f_encoder.qpack_encoder_buffer[..f_encoder.remaining_qpack_payload].to_vec()
        );
        assert_eq!(
            s_res,
            f_encoder.header_payload_buffer[..f_encoder.remaining_header_payload].to_vec()
        );
        assert_eq!(
            q_res,
            f_encoder.qpack_encoder_buffer[..f_encoder.remaining_qpack_payload].to_vec()
        );
    }
}
