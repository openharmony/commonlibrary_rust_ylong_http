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

/// Options and flags which can be used to configure `HTTP` related logic.
#[derive(Clone)]
pub(crate) struct HttpConfig {
    pub(crate) version: HttpVersion,

    #[cfg(feature = "http2")]
    pub(crate) http2_config: http2::H2Config,
}

impl HttpConfig {
    /// Creates a new, default `HttpConfig`.
    pub(crate) fn new() -> Self {
        Self {
            version: HttpVersion::Negotiate,

            #[cfg(feature = "http2")]
            http2_config: http2::H2Config::new(),
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
pub(crate) enum HttpVersion {
    /// Enforces `HTTP/1.1` or `HTTP/1.0` requests.
    Http1,

    #[cfg(feature = "http2")]
    /// Enforce `HTTP/2.0` requests without `HTTP/1.1` Upgrade or ALPN.
    Http2,

    /// Negotiate the protocol version through the ALPN.
    Negotiate,
}

#[cfg(feature = "http2")]
pub(crate) mod http2 {
    const DEFAULT_MAX_FRAME_SIZE: u32 = 2 << 13;
    const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;
    const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 16 << 20;
    // window size at the client connection level
    // The initial value specified in rfc9113 is 64kb,
    // but the default value is 1mb for performance purposes and is synchronized
    // using WINDOW_UPDATE after sending SETTINGS.
    const DEFAULT_CONN_WINDOW_SIZE: u32 = 1024 * 1024;
    // TODO Raise the default value size here.
    const DEFAULT_STREAM_WINDOW_SIZE: u32 = 64 * 1024;

    /// Settings which can be used to configure a http2 connection.
    #[derive(Clone)]
    pub(crate) struct H2Config {
        max_frame_size: u32,
        max_header_list_size: u32,
        header_table_size: u32,
        init_conn_window_size: u32,
        init_stream_window_size: u32,
        enable_push: bool,
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
            }
        }
    }
}
