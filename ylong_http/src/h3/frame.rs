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

pub const DATA_FRAME_TYPE_ID: u64 = 0x0;
pub const HEADERS_FRAME_TYPE_ID: u64 = 0x1;
pub const CANCEL_PUSH_FRAME_TYPE_ID: u64 = 0x3;
pub const SETTINGS_FRAME_TYPE_ID: u64 = 0x4;
pub const PUSH_PROMISE_FRAME_TYPE_ID: u64 = 0x5;
pub const GOAWAY_FRAME_TYPE_ID: u64 = 0x7;
pub const MAX_PUSH_FRAME_TYPE_ID: u64 = 0xD;
pub const PRIORITY_UPDATE_FRAME_REQUEST_TYPE_ID: u64 = 0xF0700;
pub const PRIORITY_UPDATE_FRAME_PUSH_TYPE_ID: u64 = 0xF0701;
pub const SETTINGS_QPACK_MAX_TABLE_CAPACITY: u64 = 0x1;
pub const SETTINGS_MAX_FIELD_SECTION_SIZE: u64 = 0x6;
pub const SETTINGS_QPACK_BLOCKED_STREAMS: u64 = 0x7;
pub const SETTINGS_ENABLE_CONNECT_PROTOCOL: u64 = 0x8;
pub const SETTINGS_H3_DATAGRAM_00: u64 = 0x276;
pub const SETTINGS_H3_DATAGRAM: u64 = 0x33;
// Permit between 16 maximally-encoded and 128 minimally-encoded SETTINGS.
const MAX_SETTINGS_PAYLOAD_SIZE: usize = 256;

#[derive(Clone)]
pub struct Frame {
    ty: u64,
    len: u64,
    payload: Payload,
}

#[derive(Clone)]
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

#[derive(Clone)]
pub struct Headers {
    headers: Vec<u8>,
    parts: Parts,
}

#[derive(Clone)]
pub struct Data {
    data: Vec<u8>,
}

#[derive(Clone)]
pub struct Settings {
    max_field_section_size: Option<u64>,
    qpack_max_table_capacity: Option<u64>,
    qpack_blocked_streams: Option<u64>,
    connect_protocol_enabled: Option<u64>,
    h3_datagram: Option<u64>,
    raw: Option<Vec<(u64, u64)>>,
}

#[derive(Clone)]
pub struct CancelPush {
    push_id: u64,
}

#[derive(Clone)]
pub struct PushPromise {
    push_id: u64,
    headers: Vec<u8>,
}

#[derive(Clone)]
pub struct GoAway {
    id: u64,
}

pub struct MaxPushId {
    push_id: u64,
}

pub struct Unknown {
    raw_type: u64,
    len: u64,
}

impl Frame {
    pub fn new(ty: u64, len: u64, payload: Payload) -> Self {
        Frame { ty, len, payload }
    }

    pub fn frame_type(&self) -> &u64 {
        &self.ty
    }

    pub fn frame_len(&self) -> &u64 {
        &self.len
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }
}

// settings结构体相当于quiche中setting结构体
impl Settings {
    /// Creates a new Settings instance containing the provided settings.
    pub fn new() -> Self {
        Settings {
            max_field_section_size: None,
            qpack_max_table_capacity: None,
            qpack_blocked_streams: None,
            connect_protocol_enabled: None,
            h3_datagram: None,
            raw: None,
        }
    }

    /// SETTINGS_HEADER_TABLE_SIZE (0x01) setting.
    pub fn max_fied_section_size(mut self, size: u64) -> Self {
        self.max_field_section_size = Some(size);
        self
    }

    /// SETTINGS_ENABLE_PUSH (0x02) setting.
    pub fn qpack_max_table_capacity(mut self, size: u64) -> Self {
        self.qpack_max_table_capacity = Some(size);
        self
    }

    /// SETTINGS_MAX_FRAME_SIZE (0x05) setting.
    pub fn qpack_block_stream(mut self, size: u64) -> Self {
        self.qpack_blocked_streams = Some(size);
        self
    }

    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06) setting.
    pub fn connect_protocol_enabled(mut self, size: u64) -> Self {
        self.connect_protocol_enabled = Some(size);
        self
    }

    pub fn h3_datagram(mut self, size: u64) -> Self {
        self.h3_datagram = Some(size);
        self
    }

    /// SETTINGS_HEADER_TABLE_SIZE (0x01) setting.
    pub fn get_max_fied_section_size(&self) -> &Option<u64> {
        &self.max_field_section_size
    }

    /// SETTINGS_ENABLE_PUSH (0x02) setting.
    pub fn get_qpack_max_table_capacity(&self) -> &Option<u64> {
        &self.qpack_max_table_capacity
    }

    /// SETTINGS_MAX_FRAME_SIZE (0x05) setting.
    pub fn get_qpack_block_stream(&self) -> &Option<u64> {
        &self.qpack_blocked_streams
    }

    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06) setting.
    pub fn get_connect_protocol_enabled(&self) -> &Option<u64> {
        &self.connect_protocol_enabled
    }

    pub fn get_h3_datagram(&self) -> &Option<u64> {
        &self.h3_datagram
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

    pub fn get_push_id(&self) -> &u64 {
        &self.push_id
    }
}

impl Headers {
    /// Creates a new Headers instance from the provided Parts.
    pub fn new(parts: Parts) -> Self {
        Headers {
            headers: vec![0; 16383],
            parts,
        }
    }

    /// Returns pseudo headers and other headers
    pub fn get_headers(&self) -> &Vec<u8> {
        &self.headers
    }

    pub fn get_part(&self) -> Parts {
        self.parts.clone()
    }
}

impl PushPromise {
    /// Creates a new PushPromise instance from the provided Parts.
    pub fn new(push_id: u64, header: Vec<u8>) -> Self {
        PushPromise {
            push_id,
            headers: header,
        }
    }

    pub fn get_push_id(&self) -> u64 {
        self.push_id
    }

    /// Returns a copy of the internal parts of the Headers.
    pub(crate) fn get_headers(&self) -> &Vec<u8> {
        &self.headers.clone()
    }
}

impl GoAway {
    /// Creates a new GoAway instance from the provided Parts.
    pub fn new(id: u64) -> Self {
        GoAway { id }
    }

    pub fn get_id(&self) -> &u64 {
        &self.id
    }
}

impl MaxPushId {
    /// Creates a new MaxPushId instance from the provided Parts.
    pub fn new(push_id: u64) -> Self {
        MaxPushId { push_id }
    }

    pub fn get_id(&self) -> &u64 {
        &self.push_id
    }
}
