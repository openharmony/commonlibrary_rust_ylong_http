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

use std::convert::TryFrom;

use crate::error::HttpError;
use crate::h2::{ErrorCode, H2Error, Parts, PseudoHeaders};
use crate::headers;

/// Mask for the END_STREAM flag.
/// When set, indicates that the sender will not send further frames for this
/// stream.
pub(crate) const END_STREAM_MASK: u8 = 0x01;

/// Mask for the RST_STREAM flag.
/// When set, indicates that the stream is being terminated.
pub(crate) const RST_STREAM_MASK: u8 = 0x03;

/// Mask for the END_HEADERS flag.
/// When set, indicates that this frame contains an entire header block and not
/// a fragment.
pub(crate) const END_HEADERS_MASK: u8 = 0x04;

/// Mask for the PADDED flag.
/// When set, indicates that the frame payload is followed by a padding field.
pub(crate) const PADDED_MASK: u8 = 0x08;

/// Mask for the HEADERS_PRIORITY flag.
/// When set, indicates that the headers frame also contains the priority
/// information.
pub(crate) const HEADERS_PRIORITY_MASK: u8 = 0x20;

/// Mask for the ACK flag
pub(crate) const ACK_MASK: u8 = 0x1;

/// HTTP/2 frame structure, including the stream ID, flags, and payload
/// information. The frame type information is represented by the `Payload`
/// type. This structure represents the fundamental unit of communication in
/// HTTP/2.
#[derive(Clone)]
pub struct Frame {
    id: StreamId,
    flags: FrameFlags,
    payload: Payload,
}

/// Enum representing the type of HTTP/2 frame.
/// Each HTTP/2 frame type serves a unique role in the communication process.
#[derive(PartialEq, Eq, Debug)]
pub enum FrameType {
    Data = 0x0,
    Headers = 0x1,
    Priority = 0x2,
    RstStream = 0x03,
    Settings = 0x4,
    PushPromise = 0x5,
    Ping = 0x6,
    Goaway = 0x7,
    WindowUpdate = 0x8,
    Continuation = 0x9,
    // add more frame types as needed
}

/// Enum representing the payload of an HTTP/2 frame.
/// The payload differs based on the type of frame.
#[derive(Clone)]
pub enum Payload {
    /// HEADERS frame payload.
    Headers(Headers),
    /// DATA frame payload.
    Data(Data),
    /// PRIORITY frame payload.
    Priority(Priority),
    /// RST_STREAM frame payload.
    RstStream(RstStream),
    /// PING frame payload.
    Ping(Ping),
    /// SETTINGS frame payload.
    Settings(Settings),
    /// GOAWAY frame payload.
    Goaway(Goaway),
    /// WINDOW_UPDATE frame payload.
    WindowUpdate(WindowUpdate),
    /// PUSH_PROMISE
    PushPromise(PushPromise),
}

/// Enum representing the different settings that can be included in a SETTINGS
/// frame. Each setting has a different role in the HTTP/2 communication
/// process.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Setting {
    /// SETTINGS_HEADER_TABLE_SIZE
    HeaderTableSize(u32),
    /// SETTINGS_ENABLE_PUSH
    EnablePush(bool),
    /// SETTINGS_MAX_CONCURRENT_STREAMS
    MaxConcurrentStreams(u32),
    /// SETTINGS_INITIAL_WINDOW_SIZE
    InitialWindowSize(u32),
    /// SETTINGS_MAX_FRAME_SIZE
    MaxFrameSize(u32),
    /// SETTINGS_MAX_HEADER_LIST_SIZE
    MaxHeaderListSize(u32),
}

type StreamId = usize;

/// HTTP/2 frame flags.
#[derive(Clone)]
pub struct FrameFlags(u8);

/// HTTP/2 HEADERS frame's payload, contains pseudo headers and other headers.
#[derive(Clone)]
pub struct Headers {
    parts: Parts,
}

/// HTTP/2 DATA frame's payload, contains all content after padding is removed.
/// The DATA frame defines the payload data of an HTTP/2 request or response.
#[derive(Clone)]
pub struct Data {
    data: Vec<u8>,
}

/// Represents the PRIORITY frame payload.
/// The PRIORITY frame specifies the sender-advised priority of a stream.
#[derive(Clone)]
pub struct Priority {
    exclusive: bool,
    stream_dependency: u32,
    weight: u8,
}

/// The RST_STREAM frame allows for immediate termination of a stream.
/// RST_STREAM is sent to request cancellation of a stream or to indicate an
/// error situation.
#[derive(Clone)]
pub struct RstStream {
    error_code: u32,
}

/// Represents the PING frame payload.
/// The PING frame is a mechanism for measuring a minimal round-trip time from
/// the sender.
#[derive(Clone)]
pub struct Ping {
    /// The opaque data of PING
    pub data: [u8; 8],
}

/// Represents the SETTINGS frame payload.
/// The SETTINGS frame conveys configuration parameters that affect how
/// endpoints communicate.
#[derive(Clone)]
pub struct Settings {
    settings: Vec<Setting>,
}

/// Represents the GOAWAY frame payload.
/// The GOAWAY frame is used to initiate shutdown of a connection or to signal
/// serious error conditions.
#[derive(Clone)]
pub struct Goaway {
    error_code: u32,
    last_stream_id: StreamId,
    debug_data: Vec<u8>,
}

/// Represents the WINDOW_UPDATE frame payload.
/// The WINDOW_UPDATE frame is used to implement flow control in HTTP/2.
#[derive(Clone)]
pub struct WindowUpdate {
    window_size_increment: u32,
}

/// Represents the PUSH_PROMISE frame payload.
/// The PUSH_PROMISE frame is used to notify the peer endpoint in advance of
/// streams the sender intends to initiate.
#[derive(Clone)]
pub struct PushPromise {
    promised_stream_id: u32,
    parts: Parts,
}

/// A Builder of SETTINGS payload.
pub struct SettingsBuilder {
    settings: Vec<Setting>,
}

impl Frame {
    /// Returns the stream identifier (`StreamId`) of the frame.
    pub fn stream_id(&self) -> StreamId {
        self.id
    }

    /// Constructs a new `Frame` with the given `StreamId`, `FrameFlags`,
    /// `Payload`.
    pub fn new(id: StreamId, flags: FrameFlags, payload: Payload) -> Self {
        Frame { id, flags, payload }
    }

    /// Returns a reference to the frame's flags (`FrameFlags`).
    pub fn flags(&self) -> &FrameFlags {
        &self.flags
    }

    /// Returns a reference to the frame's payload (`Payload`).
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    /// Returns a mutable reference to the frame's payload (`Payload`).
    /// This can be used to modify the payload of the frame.
    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }
}

impl FrameFlags {
    /// Creates a new `FrameFlags` instance with the given `flags` byte.
    pub fn new(flags: u8) -> Self {
        FrameFlags(flags)
    }

    /// Creates a new `FrameFlags` instance with no flags set.
    pub fn empty() -> Self {
        FrameFlags(0)
    }

    /// Judges the END_FLOW Flag is true.
    pub fn is_end_stream(&self) -> bool {
        self.0 & END_STREAM_MASK == END_STREAM_MASK
    }

    /// Judges the END_HEADERS Flag is true.
    pub fn is_end_headers(&self) -> bool {
        self.0 & END_HEADERS_MASK == END_HEADERS_MASK
    }

    /// Judges the PADDED Flag is true.
    pub fn is_padded(&self) -> bool {
        self.0 & PADDED_MASK == PADDED_MASK
    }

    /// Judge the ACK flag is true.
    pub fn is_ack(&self) -> bool {
        self.0 & ACK_MASK == ACK_MASK
    }

    /// Get Flags octet.
    pub fn bits(&self) -> u8 {
        self.0
    }

    /// Sets the END_STREAM flag.
    pub fn set_end_stream(&mut self, end_stream: bool) {
        if end_stream {
            self.0 |= END_STREAM_MASK;
        } else {
            self.0 &= !END_STREAM_MASK;
        }
    }

    /// Sets the END_HEADERS flag.
    pub fn set_end_headers(&mut self, end_headers: bool) {
        if end_headers {
            self.0 |= END_HEADERS_MASK;
        } else {
            self.0 &= !END_HEADERS_MASK;
        }
    }

    /// Sets the PADDED flag.
    pub fn set_padded(&mut self, padded: bool) {
        if padded {
            self.0 |= PADDED_MASK;
        } else {
            self.0 &= !PADDED_MASK;
        }
    }
}

impl Payload {
    /// Returns a reference to the Headers if the Payload is of the Headers
    /// variant. If the Payload is not of the Headers variant, returns None.
    pub(crate) fn as_headers(&self) -> Option<&Headers> {
        if let Payload::Headers(headers) = self {
            Some(headers)
        } else {
            None
        }
    }

    /// Returns the type of the frame (`FrameType`) that this payload would be
    /// associated with. The returned `FrameType` is determined based on the
    /// variant of the Payload.
    pub fn frame_type(&self) -> FrameType {
        match self {
            Payload::Headers(_) => FrameType::Headers,
            Payload::Data(_) => FrameType::Data,
            Payload::Priority(_) => FrameType::Priority,
            Payload::Ping(_) => FrameType::Ping,
            Payload::RstStream(_) => FrameType::RstStream,
            Payload::Settings(_) => FrameType::Settings,
            Payload::Goaway(_) => FrameType::Goaway,
            Payload::WindowUpdate(_) => FrameType::WindowUpdate,
            Payload::PushPromise(_) => FrameType::PushPromise,
        }
    }
}

impl Headers {
    /// Creates a new Headers instance from the provided Parts.
    pub fn new(parts: Parts) -> Self {
        Headers { parts }
    }

    /// Returns pseudo headers and other headers as tuples.
    pub fn parts(&self) -> (&PseudoHeaders, &headers::Headers) {
        self.parts.parts()
    }

    /// Returns a copy of the internal parts of the Headers.
    pub(crate) fn get_parts(&self) -> Parts {
        self.parts.clone()
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

    /// Returns the number of bytes in the `Data` payload.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

impl Settings {
    /// Creates a new Settings instance containing the provided settings.
    pub fn new(settings: Vec<Setting>) -> Self {
        Settings { settings }
    }

    /// Returns a slice of the settings.
    pub fn get_settings(&self) -> &[Setting] {
        &self.settings
    }

    /// Adds or updates a setting.
    pub(crate) fn update_setting(&mut self, setting: Setting) {
        let setting_id = setting.setting_identifier();
        if let Some(existing_setting) = self
            .settings
            .iter_mut()
            .find(|s| s.setting_identifier() == setting_id)
        {
            *existing_setting = setting;
        } else {
            self.settings.push(setting);
        }
    }

    /// Returns the total length of the settings when encoded.
    pub fn encoded_len(&self) -> usize {
        let settings_count = self.settings.len();
        // Each setting has a 2-byte ID and a 4-byte value
        let setting_size = 6;
        settings_count * setting_size
    }

    /// Returns a ACK SETTINGS frame.
    pub fn ack() -> Frame {
        Frame::new(
            0,
            FrameFlags::new(0x1),
            Payload::Settings(Settings::new(vec![])),
        )
    }
}

impl Setting {
    /// Returns the identifier associated with the setting.
    pub fn setting_identifier(&self) -> u16 {
        match self {
            Setting::HeaderTableSize(_) => 0x01,
            Setting::EnablePush(_) => 0x02,
            Setting::MaxConcurrentStreams(_) => 0x03,
            Setting::InitialWindowSize(_) => 0x04,
            Setting::MaxFrameSize(_) => 0x05,
            Setting::MaxHeaderListSize(_) => 0x06,
        }
    }
}

impl SettingsBuilder {
    /// `SettingsBuilder` constructor.
    pub fn new() -> Self {
        SettingsBuilder { settings: vec![] }
    }

    /// SETTINGS_HEADER_TABLE_SIZE (0x01) setting.
    pub fn header_table_size(mut self, size: u32) -> Self {
        self.settings.push(Setting::HeaderTableSize(size));
        self
    }

    /// SETTINGS_ENABLE_PUSH (0x02) setting.
    pub fn enable_push(mut self, is_enable: bool) -> Self {
        self.settings.push(Setting::EnablePush(is_enable));
        self
    }

    /// SETTINGS_INITIAL_WINDOW_SIZE(0x04) setting.
    pub fn initial_window_size(mut self, size: u32) -> Self {
        self.settings.push(Setting::InitialWindowSize(size));
        self
    }

    /// SETTINGS_MAX_FRAME_SIZE (0x05) setting.
    pub fn max_frame_size(mut self, size: u32) -> Self {
        self.settings.push(Setting::MaxFrameSize(size));
        self
    }

    /// SETTINGS_MAX_HEADER_LIST_SIZE (0x06) setting.
    pub fn max_header_list_size(mut self, size: u32) -> Self {
        self.settings.push(Setting::MaxHeaderListSize(size));
        self
    }

    /// Consumes the Builder and constructs a SETTINGS payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::h2::SettingsBuilder;
    ///
    /// let settings = SettingsBuilder::new()
    ///     .enable_push(true)
    ///     .header_table_size(4096)
    ///     .max_frame_size(2 << 13)
    ///     .build();
    /// ```
    pub fn build(self) -> Settings {
        Settings::new(self.settings)
    }
}

impl Default for SettingsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Goaway {
    /// Creates a new Goaway instance with the provided error code, last stream
    /// ID, and debug data.
    pub fn new(error_code: u32, last_stream_id: StreamId, debug_data: Vec<u8>) -> Self {
        Goaway {
            error_code,
            last_stream_id,
            debug_data,
        }
    }

    /// Returns a slice of the debug data.
    pub fn get_debug_data(&self) -> &[u8] {
        &self.debug_data
    }

    /// Returns the identifier of the last stream processed by the sender.
    pub fn get_last_stream_id(&self) -> StreamId {
        self.last_stream_id
    }

    /// Returns the error code.
    pub fn get_error_code(&self) -> u32 {
        self.error_code
    }

    /// Returns the total length of the Goaway frame when encoded.
    pub fn encoded_len(&self) -> usize {
        8 + self.debug_data.len() // 4-byte Last-Stream-ID + 4-byte Error Code +
                                  // Debug Data length
    }
}

impl WindowUpdate {
    /// Creates a new WindowUpdate instance with the provided window size
    /// increment.
    pub fn new(window_size_increment: u32) -> Self {
        WindowUpdate {
            window_size_increment,
        }
    }

    /// Returns the window size increment.
    pub fn get_increment(&self) -> u32 {
        self.window_size_increment
    }

    /// Returns the length of the WindowUpdate frame when encoded.
    pub fn encoded_len(&self) -> usize {
        4 // 4-byte window size increment
    }
}

impl Priority {
    /// Creates a new Priority instance with the provided exclusive flag, stream
    /// dependency, and weight.
    pub fn new(exclusive: bool, stream_dependency: u32, weight: u8) -> Self {
        Priority {
            exclusive,
            stream_dependency,
            weight,
        }
    }

    /// Returns whether the stream is exclusive.
    pub fn get_exclusive(&self) -> bool {
        self.exclusive
    }

    /// Returns the stream dependency.
    pub fn get_stream_dependency(&self) -> u32 {
        self.stream_dependency
    }

    /// Returns the weight of the stream.
    pub fn get_weight(&self) -> u8 {
        self.weight
    }
}

impl RstStream {
    /// Creates a new RstStream instance with the provided error code.
    pub fn new(error_code: u32) -> Self {
        Self { error_code }
    }

    /// Returns the error code associated with the RstStream.
    pub fn error_code(&self) -> u32 {
        self.error_code
    }

    /// GET the `ErrorCode` of `RstStream`
    pub fn error(&self, id: u32) -> Result<H2Error, H2Error> {
        Ok(H2Error::StreamError(
            id,
            ErrorCode::try_from(self.error_code)?,
        ))
    }

    /// Returns whether error code is 0.
    pub fn is_no_error(&self) -> bool {
        self.error_code == 0
    }
}

impl Ping {
    /// Creates a new Ping instance with the provided data.
    pub fn new(data: [u8; 8]) -> Self {
        Ping { data }
    }

    /// Returns the data associated with the Ping.
    pub fn data(&self) -> [u8; 8] {
        self.data
    }

    /// Returns a ACK PING frame.
    pub fn ack(ping: Ping) -> Frame {
        Frame::new(0, FrameFlags::new(0x1), Payload::Ping(ping))
    }
}

impl PushPromise {
    /// `PushPromise` constructor.
    pub fn new(promised_stream_id: u32, parts: Parts) -> Self {
        Self {
            promised_stream_id,
            parts,
        }
    }
}

#[cfg(test)]
mod ut_frame {
    use super::*;
    use crate::h2::Parts;

    /// UT test cases for `SettingsBuilder`.
    ///
    /// # Brief
    /// 1. Creates a `SettingsBuilder`.
    /// 2. Sets various setting parameters using builder methods.
    /// 3. Builds a `Settings` object.
    /// 4. Gets a reference to the underlying `Vec<Setting>` from the `Settings`
    ///    object.
    /// 5. Iterates over each setting in the `Vec<Setting>` and checks whether
    ///    it matches the expected value.
    #[test]
    fn ut_settings_builder() {
        let settings = SettingsBuilder::new()
            .header_table_size(4096)
            .enable_push(true)
            .max_frame_size(16384)
            .max_header_list_size(8192)
            .build();

        let mut setting_iter = settings.get_settings().iter();
        // Check that the first setting is as expected
        assert_eq!(setting_iter.next(), Some(&Setting::HeaderTableSize(4096)));
        // Check that the second setting is as expected
        assert_eq!(setting_iter.next(), Some(&Setting::EnablePush(true)));
        // Check that the third setting is as expected
        assert_eq!(setting_iter.next(), Some(&Setting::MaxFrameSize(16384)));
        // Check that the fourth setting is as expected
        assert_eq!(setting_iter.next(), Some(&Setting::MaxHeaderListSize(8192)));
        // Check that there are no more settings
        assert_eq!(setting_iter.next(), None);
    }

    /// UT test cases for `Ping`.
    ///
    /// # Brief
    /// 1. Creates a `Ping` instance with specific data.
    /// 2. Checks if the data of the `Ping` instance is correct.
    #[test]
    fn ut_ping() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let ping = Ping::new(data);
        assert_eq!(ping.data(), data);
    }

    /// UT test cases for `Setting`.
    ///
    /// # Brief
    /// 1. Creates a `Setting` instance for each possible variant with a
    ///    specific value.
    /// 2. Checks if the identifier of the `Setting` instance is correct.
    #[test]
    fn ut_setting() {
        let setting_header_table_size = Setting::HeaderTableSize(4096);
        assert_eq!(setting_header_table_size.setting_identifier(), 0x01);

        let setting_enable_push = Setting::EnablePush(true);
        assert_eq!(setting_enable_push.setting_identifier(), 0x02);

        let setting_max_concurrent_streams = Setting::MaxConcurrentStreams(100);
        assert_eq!(setting_max_concurrent_streams.setting_identifier(), 0x03);

        let setting_initial_window_size = Setting::InitialWindowSize(5000);
        assert_eq!(setting_initial_window_size.setting_identifier(), 0x04);

        let setting_max_frame_size = Setting::MaxFrameSize(16384);
        assert_eq!(setting_max_frame_size.setting_identifier(), 0x05);

        let setting_max_header_list_size = Setting::MaxHeaderListSize(8192);
        assert_eq!(setting_max_header_list_size.setting_identifier(), 0x06);
    }

    /// UT test cases for `Settings`.
    ///
    /// # Brief
    /// 1. Creates a `Settings` instance with a list of settings.
    /// 2. Checks if the list of settings in the `Settings` instance is correct.
    /// 3. Checks if the encoded length of the settings is correct.
    #[test]
    fn ut_settings() {
        let settings_list = vec![
            Setting::HeaderTableSize(4096),
            Setting::EnablePush(true),
            Setting::MaxFrameSize(16384),
            Setting::MaxHeaderListSize(8192),
        ];
        let settings = Settings::new(settings_list.clone());
        assert_eq!(settings.get_settings(), settings_list.as_slice());

        let encoded_len = settings.encoded_len();
        assert_eq!(encoded_len, settings_list.len() * 6);
    }

    /// UT test cases for `Payload`.
    ///
    /// # Brief
    /// 1. Creates an instance of `Payload` for each possible variant.
    /// 2. Checks if the `frame_type` of the `Payload` instance is correct.
    /// 3. Checks if `as_headers` method returns Some or None correctly.
    #[test]
    fn ut_payload() {
        let payload_headers = Payload::Headers(Headers::new(Parts::new()));
        assert_eq!(payload_headers.frame_type(), FrameType::Headers);
        assert!(payload_headers.as_headers().is_some());

        let payload_data = Payload::Data(Data::new(b"hh".to_vec()));
        assert_eq!(payload_data.frame_type(), FrameType::Data);
        assert!(payload_data.as_headers().is_none());

        let payload_priority = Payload::Priority(Priority::new(true, 1, 10));
        assert_eq!(payload_priority.frame_type(), FrameType::Priority);
        assert!(payload_priority.as_headers().is_none());

        let payload_rst_stream = Payload::RstStream(RstStream::new(20));
        assert_eq!(payload_rst_stream.frame_type(), FrameType::RstStream);
        assert!(payload_rst_stream.as_headers().is_none());

        let payload_ping = Payload::Ping(Ping::new([0; 8]));
        assert_eq!(payload_ping.frame_type(), FrameType::Ping);
        assert!(payload_ping.as_headers().is_none());

        let payload_goaway = Payload::Goaway(Goaway::new(30, 20, vec![0; 0]));
        assert_eq!(payload_goaway.frame_type(), FrameType::Goaway);
        assert!(payload_goaway.as_headers().is_none());

        let payload_window_update = Payload::WindowUpdate(WindowUpdate::new(1024));
        assert_eq!(payload_window_update.frame_type(), FrameType::WindowUpdate);
        assert!(payload_window_update.as_headers().is_none());

        let payload_push_promise = Payload::PushPromise(PushPromise::new(3, Parts::new()));
        assert_eq!(payload_push_promise.frame_type(), FrameType::PushPromise);
        assert!(payload_push_promise.as_headers().is_none());
    }
}
