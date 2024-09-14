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

use crate::h3::parts::Parts;

/// Data Frame type code.
pub const DATA_FRAME_TYPE: u64 = 0x0;
/// HEADERS Frame type code.
pub const HEADERS_FRAME_TYPE: u64 = 0x1;
/// CANCEL_PUSH Frame type code.
pub const CANCEL_PUSH_FRAME_TYPE: u64 = 0x3;
/// SETTINGS Frame type code.
pub const SETTINGS_FRAME_TYPE: u64 = 0x4;
/// PUSH_PROMISE Frame type code.
pub const PUSH_PROMISE_FRAME_TYPE: u64 = 0x5;
/// GOAWAY Frame type code.
pub const GOAWAY_FRAME_TYPE: u64 = 0x7;
/// MAX_PUSH_ID Frame type code.
pub const MAX_PUSH_ID_FRAME_TYPE: u64 = 0xD;
/// SETTING_QPACK_MAX_TABLE_CAPACITY setting code.
pub const SETTING_QPACK_MAX_TABLE_CAPACITY: u64 = 0x1;
/// SETTING_MAX_FIELD_SECTION_SIZE setting code.
pub const SETTING_MAX_FIELD_SECTION_SIZE: u64 = 0x6;
/// SETTING_QPACK_BLOCKED_STREAMS setting code.
pub const SETTING_QPACK_BLOCKED_STREAMS: u64 = 0x7;
/// SETTING_ENABLE_CONNECT_PROTOCOL setting code.
pub const SETTING_ENABLE_CONNECT_PROTOCOL: u64 = 0x8;
/// SETTING_H3_DATAGRAM setting code.
pub const SETTING_H3_DATAGRAM: u64 = 0x33;
/// MAX_SETTING_PAYLOAD_SIZE setting code.
// Permit between 16 maximally-encoded and 128 minimally-encoded SETTINGS.
const MAX_SETTING_PAYLOAD_SIZE: usize = 256;

/// Http3 frame definition.
#[derive(Clone, Debug)]
pub struct Frame {
    ty: u64,
    payload: Payload,
}

/// Http3 frame payload.
#[derive(Clone, Debug)]
pub enum Payload {
    /// HEADERS frame payload.
    Headers(Headers),
    /// DATA frame payload.
    Data(Data),
    /// SETTINGS frame payload.
    Settings(Settings),
    /// CancelPush frame payload.
    CancelPush(CancelPush),
    /// PushPromise frame payload.
    PushPromise(PushPromise),
    /// GOAWAY frame payload.
    Goaway(GoAway),
    /// MaxPushId frame payload.
    MaxPushId(MaxPushId),
    /// Unknown frame payload.
    Unknown(Unknown),
}

/// Http3 Headers frame payload, which also contains instructions to send when
/// decoding.
#[derive(Clone, Debug)]
pub struct Headers {
    parts: Parts,
    ins: Option<Vec<u8>>,
}

/// Http3 Data frame payload, containing the body data.
#[derive(Clone, Debug)]
pub struct Data {
    data: Vec<u8>,
}

/// Http3 Settings frame payload.
#[derive(Clone, Default, Debug)]
pub struct Settings {
    max_field_section_size: Option<u64>,
    qpack_max_table_capacity: Option<u64>,
    qpack_blocked_streams: Option<u64>,
    connect_protocol_enabled: Option<u64>,
    h3_datagram: Option<u64>,
    additional: Option<Vec<(u64, u64)>>,
}

/// Http3 CancelPush frame payload.
#[derive(Clone, Debug)]
pub struct CancelPush {
    push_id: u64,
}

/// Http3 PushPromise frame payload.
#[derive(Clone, Debug)]
pub struct PushPromise {
    push_id: u64,
    parts: Parts,
    ins: Option<Vec<u8>>,
}

/// Http3 GoAway frame payload.
#[derive(Clone, Debug)]
pub struct GoAway {
    id: u64,
}

/// Http3 MaxPushId frame payload.
#[derive(Clone, Debug)]
pub struct MaxPushId {
    push_id: u64,
}

/// Http3 Unknown frame payload.
#[derive(Clone, Debug)]
pub struct Unknown {
    raw_type: u64,
    len: u64,
}

impl Frame {
    /// Constructs a Frame with type and payload.
    pub fn new(ty: u64, payload: Payload) -> Self {
        Frame { ty, payload }
    }

    /// Gets frame type.
    pub fn frame_type(&self) -> &u64 {
        &self.ty
    }

    /// Gets frame payload.
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    /// Gets a mutable frame payload of current frame.
    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }
}

impl Settings {
    /// Sets SETTINGS_HEADER_TABLE_SIZE (0x01) setting.
    pub fn set_max_field_section_size(&mut self, size: u64) {
        self.max_field_section_size = Some(size)
    }

    /// Sets SETTINGS_ENABLE_PUSH (0x02) setting.
    pub fn set_qpack_max_table_capacity(&mut self, size: u64) {
        self.qpack_max_table_capacity = Some(size)
    }

    /// Sets SETTINGS_MAX_FRAME_SIZE (0x05) setting.
    pub fn set_qpack_block_stream(&mut self, size: u64) {
        self.qpack_blocked_streams = Some(size)
    }

    /// Sets SETTINGS_MAX_HEADER_LIST_SIZE (0x06) setting.
    pub fn set_connect_protocol_enabled(&mut self, size: u64) {
        self.connect_protocol_enabled = Some(size)
    }

    /// Sets SETTINGS_H3_DATAGRAM setting.
    pub fn set_h3_datagram(&mut self, size: u64) {
        self.h3_datagram = Some(size);
    }

    /// Sets additional settings.
    pub fn set_additional(&mut self, addition: Vec<(u64, u64)>) {
        self.additional = Some(addition)
    }

    /// Gets SETTINGS_MAX_FIELD_SECTION_SIZE setting.
    pub fn max_fied_section_size(&self) -> Option<u64> {
        self.max_field_section_size
    }

    /// Gets SETTINGS_QPACK_MAX_TABLE_CAPACITY setting.
    pub fn qpack_max_table_capacity(&self) -> Option<u64> {
        self.qpack_max_table_capacity
    }

    /// Gets SETTINGS_QPACK_BLOCKED_STREAMS setting.
    pub fn qpack_block_stream(&self) -> Option<u64> {
        self.qpack_blocked_streams
    }

    /// Gets SETTINGS_ENABLE_CONNECT_PROTOCOL setting.
    pub fn connect_protocol_enabled(&self) -> Option<u64> {
        self.connect_protocol_enabled
    }

    /// Gets SETTINGS_H3_DATAGRAM setting.
    pub fn h3_datagram(&self) -> Option<u64> {
        self.h3_datagram
    }

    /// Gets additional settings.
    pub fn additional(&self) -> &Option<Vec<(u64, u64)>> {
        &self.additional
    }
}

impl Data {
    /// Creates a new Data instance containing the provided data.
    pub fn new(data: Vec<u8>) -> Self {
        Data { data }
    }

    /// Return the `Vec` that contains the data payload.
    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl CancelPush {
    /// Creates a new CancelPush instance from the provided Parts.
    pub fn new(id: u64) -> Self {
        CancelPush { push_id: id }
    }

    /// Gets push id of CancelPush payload.
    pub fn get_push_id(&self) -> &u64 {
        &self.push_id
    }
}

impl Headers {
    /// Creates a new Headers instance from the provided Parts.
    pub fn new(parts: Parts) -> Self {
        Headers { parts, ins: None }
    }

    /// Gets the instructions generated by qpack decoder after decoding headers
    /// frame.
    pub fn get_instruction(&self) -> &Option<Vec<u8>> {
        &self.ins
    }

    /// Gets headers part of Headers frame payload.
    pub fn get_part(&self) -> Parts {
        self.parts.clone()
    }

    pub(crate) fn set_instruction(&mut self, buf: Vec<u8>) {
        self.ins = Some(buf)
    }
}

impl PushPromise {
    /// Creates a new PushPromise instance from the provided Parts.
    pub fn new(push_id: u64, parts: Parts) -> Self {
        PushPromise {
            push_id,
            parts,
            ins: None,
        }
    }

    /// Gets push id of PushPromise payload.
    pub fn get_push_id(&self) -> u64 {
        self.push_id
    }

    /// Returns a copy of the internal parts of the Headers.
    pub(crate) fn get_parts(&self) -> &Parts {
        &self.parts
    }

    pub(crate) fn set_instruction(&mut self, buf: Vec<u8>) {
        self.ins = Some(buf)
    }
}

impl GoAway {
    /// Creates a new GoAway instance from the provided Parts.
    pub fn new(id: u64) -> Self {
        GoAway { id }
    }

    /// Gets go away stream id.
    pub fn get_id(&self) -> &u64 {
        &self.id
    }
}

impl MaxPushId {
    /// Creates a new MaxPushId instance from the provided Parts.
    pub fn new(push_id: u64) -> Self {
        MaxPushId { push_id }
    }

    /// Gets allowed max push stream id.
    pub fn get_id(&self) -> &u64 {
        &self.push_id
    }
}
