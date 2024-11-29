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

//! HTTP configure module.
use crate::util::progress::SpeedConfig;
#[cfg(feature = "http3")]
use crate::ErrorKind;

/// Options and flags which can be used to configure `HTTP` related logic.
#[derive(Clone)]
pub(crate) struct HttpConfig {
    pub(crate) version: HttpVersion,
    pub(crate) speed_config: SpeedConfig,

    #[cfg(feature = "http1_1")]
    pub(crate) http1_config: http1::H1Config,

    #[cfg(feature = "http2")]
    pub(crate) http2_config: http2::H2Config,

    #[cfg(feature = "http3")]
    pub(crate) http3_config: http3::H3Config,
}

impl HttpConfig {
    /// Creates a new, default `HttpConfig`.
    pub(crate) fn new() -> Self {
        Self {
            version: HttpVersion::Negotiate,
            speed_config: SpeedConfig::none(),
            #[cfg(feature = "http1_1")]
            http1_config: http1::H1Config::default(),
            #[cfg(feature = "http2")]
            http2_config: http2::H2Config::new(),
            #[cfg(feature = "http3")]
            http3_config: http3::H3Config::new(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// `HTTP` version to use.
#[derive(PartialEq, Eq, Clone)]
pub enum HttpVersion {
    /// Enforces `HTTP/1.1` or `HTTP/1.0` requests.
    Http1,

    #[cfg(feature = "http2")]
    /// Enforce `HTTP/2.0` requests without `HTTP/1.1` Upgrade or ALPN.
    Http2,

    #[cfg(feature = "http3")]
    /// Enforces `HTTP/3` requests.
    Http3,

    /// Negotiate the protocol version through the ALPN.
    Negotiate,
}

#[cfg(feature = "http3")]
impl TryFrom<&[u8]> for HttpVersion {
    type Error = ErrorKind;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value == b"h1" {
            return Ok(HttpVersion::Http1);
        }
        #[cfg(feature = "http2")]
        if value == b"h2" {
            return Ok(HttpVersion::Http2);
        }
        if value == b"h3" {
            Ok(HttpVersion::Http3)
        } else {
            Err(ErrorKind::Other)
        }
    }
}

#[cfg(feature = "http1_1")]
pub(crate) mod http1 {
    const DEFAULT_MAX_CONN_NUM: usize = 6;

    #[derive(Clone)]
    pub(crate) struct H1Config {
        max_conn_num: usize,
    }

    impl H1Config {
        pub(crate) fn set_max_conn_num(&mut self, num: usize) {
            self.max_conn_num = num
        }

        pub(crate) fn max_conn_num(&self) -> usize {
            self.max_conn_num
        }
    }

    impl Default for H1Config {
        fn default() -> Self {
            Self {
                max_conn_num: DEFAULT_MAX_CONN_NUM,
            }
        }
    }
}

#[cfg(feature = "http2")]
pub(crate) mod http2 {
    const DEFAULT_MAX_FRAME_SIZE: u32 = 16 * 1024;
    const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;
    const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 16 * 1024;
    // window size at the client connection level
    // The initial value specified in rfc9113 is 64kb,
    // but the default value is 1mb for performance purposes and is synchronized
    // using WINDOW_UPDATE after sending SETTINGS.
    const DEFAULT_CONN_WINDOW_SIZE: u32 = 10 * 1024 * 1024;
    const DEFAULT_STREAM_WINDOW_SIZE: u32 = 2 * 1024 * 1024;

    /// Settings which can be used to configure a http2 connection.
    #[derive(Clone)]
    pub(crate) struct H2Config {
        max_frame_size: u32,
        max_header_list_size: u32,
        header_table_size: u32,
        init_conn_window_size: u32,
        init_stream_window_size: u32,
        enable_push: bool,
        allowed_cache_frame_size: usize,
        use_huffman: bool,
    }

    impl H2Config {
        /// `H2Config` constructor.
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Sets the SETTINGS_MAX_FRAME_SIZE.
        pub(crate) fn set_max_frame_size(&mut self, size: u32) {
            self.max_frame_size = size;
        }

        /// Sets the SETTINGS_MAX_HEADER_LIST_SIZE.
        pub(crate) fn set_max_header_list_size(&mut self, size: u32) {
            self.max_header_list_size = size;
        }

        /// Sets the SETTINGS_HEADER_TABLE_SIZE.
        pub(crate) fn set_header_table_size(&mut self, size: u32) {
            self.header_table_size = size;
        }

        pub(crate) fn set_conn_window_size(&mut self, size: u32) {
            self.init_conn_window_size = size;
        }

        pub(crate) fn set_stream_window_size(&mut self, size: u32) {
            self.init_stream_window_size = size;
        }

        pub(crate) fn set_allowed_cache_frame_size(&mut self, size: usize) {
            self.allowed_cache_frame_size = size;
        }

        pub(crate) fn set_use_huffman_coding(&mut self, use_huffman: bool) {
            self.use_huffman = use_huffman;
        }

        /// Gets the SETTINGS_MAX_FRAME_SIZE.
        pub(crate) fn max_frame_size(&self) -> u32 {
            self.max_frame_size
        }

        /// Gets the SETTINGS_MAX_HEADER_LIST_SIZE.
        pub(crate) fn max_header_list_size(&self) -> u32 {
            self.max_header_list_size
        }

        /// Gets the SETTINGS_MAX_FRAME_SIZE.
        pub(crate) fn header_table_size(&self) -> u32 {
            self.header_table_size
        }

        pub(crate) fn enable_push(&self) -> bool {
            self.enable_push
        }

        pub(crate) fn conn_window_size(&self) -> u32 {
            self.init_conn_window_size
        }

        pub(crate) fn stream_window_size(&self) -> u32 {
            self.init_stream_window_size
        }

        pub(crate) fn allowed_cache_frame_size(&self) -> usize {
            self.allowed_cache_frame_size
        }

        pub(crate) fn use_huffman_coding(&self) -> bool {
            self.use_huffman
        }
    }

    impl Default for H2Config {
        fn default() -> Self {
            Self {
                max_frame_size: DEFAULT_MAX_FRAME_SIZE,
                max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
                header_table_size: DEFAULT_HEADER_TABLE_SIZE,
                init_conn_window_size: DEFAULT_CONN_WINDOW_SIZE,
                init_stream_window_size: DEFAULT_STREAM_WINDOW_SIZE,
                enable_push: false,
                allowed_cache_frame_size: 5,
                use_huffman: true,
            }
        }
    }
}

#[cfg(feature = "http3")]
pub(crate) mod http3 {
    const DEFAULT_MAX_FIELD_SECTION_SIZE: u64 = 16 * 1024;
    const DEFAULT_QPACK_MAX_TABLE_CAPACITY: u64 = 16 * 1024;
    const DEFAULT_QPACK_BLOCKED_STREAMS: u64 = 10;

    // todo: which settings should be pub to user
    #[derive(Clone)]
    pub(crate) struct H3Config {
        max_field_section_size: u64,
        qpack_max_table_capacity: u64,
        qpack_blocked_streams: u64,
        connect_protocol_enabled: Option<u64>,
        additional_settings: Option<Vec<(u64, u64)>>,
    }

    impl H3Config {
        /// `H3Config` constructor.

        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn set_max_field_section_size(&mut self, size: u64) {
            self.max_field_section_size = size;
        }

        pub(crate) fn set_qpack_max_table_capacity(&mut self, size: u64) {
            self.qpack_max_table_capacity = size;
        }

        pub(crate) fn set_qpack_blocked_streams(&mut self, size: u64) {
            self.qpack_blocked_streams = size;
        }

        #[allow(unused)]
        fn set_connect_protocol_enabled(&mut self, size: u64) {
            self.connect_protocol_enabled = Some(size);
        }

        #[allow(unused)]
        fn insert_additional_settings(&mut self, key: u64, value: u64) {
            if let Some(vec) = &mut self.additional_settings {
                vec.push((key, value));
            } else {
                self.additional_settings = Some(vec![(key, value)]);
            }
        }

        pub(crate) fn max_field_section_size(&self) -> u64 {
            self.max_field_section_size
        }

        pub(crate) fn qpack_max_table_capacity(&self) -> u64 {
            self.qpack_max_table_capacity
        }

        pub(crate) fn qpack_blocked_streams(&self) -> u64 {
            self.qpack_blocked_streams
        }

        #[allow(unused)]
        fn connect_protocol_enabled(&mut self) -> Option<u64> {
            self.connect_protocol_enabled
        }

        #[allow(unused)]
        fn additional_settings(&mut self) -> Option<Vec<(u64, u64)>> {
            self.additional_settings.clone()
        }
    }

    impl Default for H3Config {
        fn default() -> Self {
            Self {
                max_field_section_size: DEFAULT_MAX_FIELD_SECTION_SIZE,
                qpack_max_table_capacity: DEFAULT_QPACK_MAX_TABLE_CAPACITY,
                qpack_blocked_streams: DEFAULT_QPACK_BLOCKED_STREAMS,
                connect_protocol_enabled: None,
                additional_settings: None,
            }
        }
    }
}
