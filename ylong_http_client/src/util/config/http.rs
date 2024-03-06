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
            version: HttpVersion::Http1,

            #[cfg(feature = "http2")]
            http2_config: http2::H2Config::default(),
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
    /// Enforce `HTTP/2.0` requests without `HTTP/1.1` Upgrade.
    Http2PriorKnowledge,
}

#[cfg(feature = "http2")]
pub(crate) mod http2 {
    const DEFAULT_MAX_FRAME_SIZE: u32 = 2 << 13;
    const DEFAULT_HEADER_TABLE_SIZE: u32 = 4096;
    const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 16 << 20;

    /// Settings which can be used to configure a http2 connection.
    #[derive(Clone)]
    pub(crate) struct H2Config {
        pub(crate) max_frame_size: u32,
        pub(crate) max_header_list_size: u32,
        pub(crate) header_table_size: u32,
    }

    impl H2Config {
        /// `H2Config` constructor.
        pub(crate) fn new() -> Self {
            Self::default()
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
    }

    impl Default for H2Config {
        fn default() -> Self {
            Self {
                max_frame_size: DEFAULT_MAX_FRAME_SIZE,
                max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
                header_table_size: DEFAULT_HEADER_TABLE_SIZE,
            }
        }
    }
}
