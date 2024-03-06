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

use super::{Console, DownloadConfig, DownloadOperator, Downloader};
use crate::async_impl::Response;
use crate::util::{SpeedLimit, Timeout};

/// A builder that can create a `Downloader`.
///
/// You can use this builder to build a `Downloader` step by step.
///
/// # Examples
///
/// ```
/// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
///
/// # async fn create_a_downloader(body: Response) {
/// let downloader = DownloaderBuilder::new().body(body).console().build();
/// # }
/// ```
pub struct DownloaderBuilder<S> {
    state: S,
}

/// A state indicates that `DownloaderBuilder` wants a body that needs to be
/// downloaded.
pub struct WantsBody;

impl DownloaderBuilder<WantsBody> {
    /// Creates a `DownloaderBuilder` in the `WantsBody` state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::DownloaderBuilder;
    ///
    /// let builder = DownloaderBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self { state: WantsBody }
    }

    /// Sets a body part that needs to be downloaded by the downloader.
    ///
    /// Then the `DownloaderBuilder` will switch to `WantsOperator` state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
    ///
    /// # async fn set_body(body: Response) {
    /// let builder = DownloaderBuilder::new().body(body);
    /// # }
    /// ```
    pub fn body(self, body: Response) -> DownloaderBuilder<WantsOperator> {
        DownloaderBuilder {
            state: WantsOperator { body },
        }
    }
}

impl Default for DownloaderBuilder<WantsBody> {
    fn default() -> Self {
        Self::new()
    }
}

/// A state indicates that `DownloaderBuilder` wants an `DownloadOperator`.
pub struct WantsOperator {
    body: Response,
}

impl DownloaderBuilder<WantsOperator> {
    /// Sets a customized `DownloaderBuilder`.
    ///
    /// Then the `DownloaderBuilder` will switch to `WantsConfig` state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::pin::Pin;
    /// # use std::task::{Context, Poll};
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, DownloadOperator, HttpBody, Response};
    /// # use ylong_http_client::HttpClientError;
    ///
    /// # async fn set_downloader_operator(body: Response) {
    /// struct MyOperator;
    ///
    /// impl DownloadOperator for MyOperator {
    ///     fn poll_download(
    ///         self: Pin<&mut Self>,
    ///         cx: &mut Context<'_>,
    ///         data: &[u8]
    ///     ) -> Poll<Result<usize, HttpClientError>> {
    ///         todo!()
    ///     }
    ///
    ///     fn poll_progress(
    ///         self: Pin<&mut Self>,
    ///         cx: &mut Context<'_>,
    ///         downloaded: u64,
    ///         total: Option<u64>
    ///     ) -> Poll<Result<(), HttpClientError>> {
    ///         todo!()
    ///     }
    /// }
    ///
    /// let builder = DownloaderBuilder::new().body(body).operator(MyOperator);
    /// # }
    /// ```
    pub fn operator<T: DownloadOperator>(self, operator: T) -> DownloaderBuilder<WantsConfig<T>> {
        DownloaderBuilder {
            state: WantsConfig {
                body: self.state.body,
                operator,
                config: DownloadConfig::default(),
            },
        }
    }

    /// Sets a `Console` to this `Downloader`. The download result and progress
    /// will be displayed on the console.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
    ///
    /// # async fn set_console(body: Response) {
    /// let builder = DownloaderBuilder::new().body(body).console();
    /// # }
    /// ```
    pub fn console(self) -> DownloaderBuilder<WantsConfig<Console>> {
        DownloaderBuilder {
            state: WantsConfig {
                body: self.state.body,
                operator: Console,
                config: DownloadConfig::default(),
            },
        }
    }
}

/// A state indicates that `DownloaderBuilder` wants some configurations.
pub struct WantsConfig<T: DownloadOperator> {
    body: Response,
    operator: T,
    config: DownloadConfig,
}

impl<T: DownloadOperator> DownloaderBuilder<WantsConfig<T>> {
    /// Sets the timeout for downloading body.
    ///
    /// Default is `Timeout::none()`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
    /// # use ylong_http_client::Timeout;
    ///
    /// # async fn set_timeout(body: Response) {
    /// let builder = DownloaderBuilder::new()
    ///     .body(body)
    ///     .console()
    ///     .timeout(Timeout::none());
    /// # }
    /// ```
    pub fn timeout(mut self, timeout: Timeout) -> Self {
        self.state.config.timeout = timeout;
        self
    }

    /// Sets the speed limit for downloading body.
    ///
    /// Default is `SpeedLimit::none()`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
    /// # use ylong_http_client::SpeedLimit;
    ///
    /// # async fn set_timeout(body: Response) {
    /// let builder = DownloaderBuilder::new()
    ///     .body(body)
    ///     .console()
    ///     .speed_limit(SpeedLimit::none());
    /// # }
    /// ```
    pub fn speed_limit(mut self, speed_limit: SpeedLimit) -> Self {
        self.state.config.speed_limit = speed_limit;
        self
    }

    /// Returns a `Downloader` that uses this `DownloaderBuilder` configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{DownloaderBuilder, Downloader, HttpBody, Response};
    ///
    /// # async fn build_downloader(body: Response) {
    /// let downloader = DownloaderBuilder::new().body(body).console().build();
    /// # }
    /// ```
    pub fn build(self) -> Downloader<T> {
        Downloader {
            body: self.state.body,
            operator: self.state.operator,
            config: self.state.config,
            info: None,
        }
    }
}
