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

use super::{Console, UploadConfig, UploadOperator, Uploader};
use crate::async_impl::MultiPart;
use crate::runtime::AsyncRead;

/// A builder that can create a `Uploader`.
///
/// You can use this builder to build a `Uploader` step by step.
///
/// # Examples
///
/// ```
/// # use ylong_http_client::async_impl::{UploaderBuilder, Uploader};
///
/// let uploader = UploaderBuilder::new()
///     .reader("HelloWorld".as_bytes())
///     .console()
///     .build();
/// ```
pub struct UploaderBuilder<S> {
    state: S,
}

/// A state indicates that `UploaderBuilder` wants a `Reader`.
pub struct WantsReader;

impl UploaderBuilder<WantsReader> {
    /// Creates a `UploaderBuilder` in the `WantsReader` state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::UploaderBuilder;
    ///
    /// let builder = UploaderBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self { state: WantsReader }
    }

    /// Sets a reader that needs to be read.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::UploaderBuilder;
    ///
    /// let builder = UploaderBuilder::new().reader("HelloWorld".as_bytes());
    /// ```
    pub fn reader<R: AsyncRead>(self, reader: R) -> UploaderBuilder<WantsOperator<R>> {
        UploaderBuilder {
            state: WantsOperator {
                reader,
                config: UploadConfig::default(),
            },
        }
    }

    /// Sets a `multipart` that needs to be read. The size of the multipart will
    /// be set automatically if it contains.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::UploaderBuilder;
    ///
    /// let builder = UploaderBuilder::new().reader("HelloWorld".as_bytes());
    /// ```
    pub fn multipart(self, reader: MultiPart) -> UploaderBuilder<WantsOperator<MultiPart>> {
        let total_bytes = reader.total_bytes();
        UploaderBuilder {
            state: WantsOperator {
                reader,
                config: UploadConfig { total_bytes },
            },
        }
    }
}

impl Default for UploaderBuilder<WantsReader> {
    fn default() -> Self {
        Self::new()
    }
}

/// A state indicates that `UploaderBuilder` wants an `UploadOperator`.
pub struct WantsOperator<R> {
    reader: R,
    config: UploadConfig,
}

impl<R: AsyncRead> UploaderBuilder<WantsOperator<R>> {
    /// Sets a customized `UploaderOperator`.
    ///
    /// Then the `UploaderBuilder` will switch to `WantsConfig` state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::pin::Pin;
    /// # use std::task::{Context, Poll};
    /// # use ylong_http_client::async_impl::{UploaderBuilder, Uploader, UploadOperator, Response};
    /// # use ylong_http_client::HttpClientError;
    ///
    /// struct MyOperator;
    ///
    /// impl UploadOperator for MyOperator {
    ///     fn poll_progress(
    ///         self: Pin<&mut Self>,
    ///         cx: &mut Context<'_>,
    ///         uploaded: u64,
    ///         total: Option<u64>,
    ///     ) -> Poll<Result<(), HttpClientError>> {
    ///         todo!()
    ///     }
    /// }
    ///
    /// let builder = UploaderBuilder::new()
    ///     .reader("HelloWorld".as_bytes())
    ///     .operator(MyOperator);
    /// ```
    pub fn operator<T: UploadOperator>(self, operator: T) -> UploaderBuilder<WantsConfig<R, T>> {
        UploaderBuilder {
            state: WantsConfig {
                reader: self.state.reader,
                operator,
                config: self.state.config,
            },
        }
    }

    /// Sets a `Console` to this `Uploader`. The download result and progress
    /// will be displayed on the console.
    ///
    /// The `Console` needs a `Reader` to display.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{UploaderBuilder, Uploader, Response};
    ///
    /// let builder = UploaderBuilder::new()
    ///     .reader("HelloWorld".as_bytes())
    ///     .console();
    /// ```
    pub fn console(self) -> UploaderBuilder<WantsConfig<R, Console>> {
        UploaderBuilder {
            state: WantsConfig {
                reader: self.state.reader,
                operator: Console,
                config: self.state.config,
            },
        }
    }
}

/// A state indicates that `UploaderBuilder` wants some configurations.
pub struct WantsConfig<R, T> {
    reader: R,
    operator: T,
    config: UploadConfig,
}

impl<R, T> UploaderBuilder<WantsConfig<R, T>> {
    /// Sets the total bytes of the uploaded content.
    ///
    /// Default is `None` which means that you don't know the size of the
    /// content.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{UploaderBuilder, Uploader};
    ///
    /// let builder = UploaderBuilder::new()
    ///     .reader("HelloWorld".as_bytes())
    ///     .console()
    ///     .total_bytes(Some(10));
    /// ```
    pub fn total_bytes(mut self, total_bytes: Option<u64>) -> Self {
        self.state.config.total_bytes = total_bytes;
        self
    }

    /// Returns a `Uploader` that uses this `UploaderBuilder` configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{UploaderBuilder, Uploader, Response};
    ///
    /// let uploader = UploaderBuilder::new()
    ///     .reader("HelloWorld".as_bytes())
    ///     .console()
    ///     .build();
    /// ```
    pub fn build(self) -> Uploader<R, T> {
        Uploader {
            reader: self.state.reader,
            operator: self.state.operator,
            config: self.state.config,
            info: None,
        }
    }
}
