/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

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
            version: HttpVersion::Http11,

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
    /// Enforces `HTTP/1.1` requests.
    Http11,

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
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::H2Config;
    ///
    /// let config = H2Config::new()
    /// .set_header_table_size(4096)
    /// .set_max_header_list_size(16 << 20)
    /// .set_max_frame_size(2 << 13);
    /// ```
    #[derive(Clone)]
    pub struct H2Config {
        max_frame_size: u32,
        max_header_list_size: u32,
        header_table_size: u32,
    }

    impl H2Config {
        /// `H2Config` constructor.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new();
        /// ```
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the SETTINGS_MAX_FRAME_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_max_frame_size(2 << 13);
        /// ```
        pub fn set_max_frame_size(mut self, size: u32) -> Self {
            self.max_frame_size = size;
            self
        }

        /// Sets the SETTINGS_MAX_HEADER_LIST_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_max_header_list_size(16 << 20);
        /// ```
        pub fn set_max_header_list_size(mut self, size: u32) -> Self {
            self.max_header_list_size = size;
            self
        }

        /// Sets the SETTINGS_HEADER_TABLE_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_max_header_list_size(4096);
        /// ```
        pub fn set_header_table_size(mut self, size: u32) -> Self {
            self.header_table_size = size;
            self
        }

        /// Gets the SETTINGS_MAX_FRAME_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_max_frame_size(2 << 13);
        /// assert_eq!(config.max_frame_size(), 2 << 13);
        /// ```
        pub fn max_frame_size(&self) -> u32 {
            self.max_frame_size
        }

        /// Gets the SETTINGS_MAX_HEADER_LIST_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_max_header_list_size(16 << 20);
        /// assert_eq!(config.max_header_list_size(), 16 << 20);
        /// ```
        pub fn max_header_list_size(&self) -> u32 {
            self.max_header_list_size
        }

        /// Gets the SETTINGS_MAX_FRAME_SIZE.
        ///
        /// # Examples
        ///
        /// ```
        /// use ylong_http_client::util::H2Config;
        ///
        /// let config = H2Config::new()
        ///     .set_header_table_size(4096);
        /// assert_eq!(config.header_table_size(), 4096);
        /// ```
        pub fn header_table_size(&self) -> u32 {
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
