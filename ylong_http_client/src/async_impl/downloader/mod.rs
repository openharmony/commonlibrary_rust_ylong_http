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

mod builder;
mod operator;

use builder::WantsBody;
use operator::Console;

pub use builder::DownloaderBuilder;
pub use operator::{DownloadFuture, DownloadOperator, ProgressFuture};

// TODO: Adapter, use Response<HttpBody> later.
use crate::async_impl::Response;
use crate::{ErrorKind, HttpClientError, SpeedLimit, Timeout};
use std::time::Instant;
use ylong_http::body::async_impl::Body;

/// A downloader that can help you download the response body.
///
/// A `Downloader` provides a template method for downloading the body and
/// needs to use a structure that implements [`DownloadOperator`] trait to read
/// the body.
///
/// The `DownloadOperator` trait provides two kinds of methods - [`download`]
/// and [`progress`], where:
///
/// - `download` methods are responsible for reading and copying the body to
/// certain places.
///
/// - `progress` methods are responsible for progress display.
///
/// You only need to provide a structure that implements the `DownloadOperator`
/// trait to complete the download process.
///
/// A default structure `Console` which implements `DownloadOperator` is
/// provided to show download message on console. You can use
/// `Downloader::console` to build a `Downloader` which based on it.
///
/// [`DownloadOperator`]: DownloadOperator
/// [`download`]: DownloadOperator::download
/// [`progress`]: DownloadOperator::progress
///
/// # Examples
///
/// `Console`:
/// ```no_run
/// # use ylong_http_client::async_impl::{Downloader, HttpBody, Response};
///
/// # async fn download_and_show_progress_on_console(response: Response) {
/// // Creates a default `Downloader` that show progress on console.
/// let mut downloader = Downloader::console(response);
/// let _ = downloader.download().await;
/// # }
/// ```
///
/// `Custom`:
/// ```no_run
/// # use std::pin::Pin;
/// # use std::task::{Context, Poll};
/// # use ylong_http_client::async_impl::{Downloader, DownloadOperator, HttpBody, Response};
/// # use ylong_http_client::{HttpClientError, SpeedLimit, Timeout};
///
/// # async fn download_and_show_progress(response: Response) {
/// // Customizes your own `DownloadOperator`.
/// struct MyDownloadOperator;
///
/// impl DownloadOperator for MyDownloadOperator {
///     fn poll_download(
///         self: Pin<&mut Self>,
///         cx: &mut Context<'_>,
///         data: &[u8],
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
///         // Writes your customize method.
///         todo!()
///     }
/// }
///
/// // Creates a default `Downloader` based on `MyDownloadOperator`.
/// // Configures your downloader by using `DownloaderBuilder`.
/// let mut downloader = Downloader::builder()
///     .body(response)
///     .operator(MyDownloadOperator)
///     .timeout(Timeout::none())
///     .speed_limit(SpeedLimit::none())
///     .build();
/// let _ = downloader.download().await;
/// # }
/// ```
pub struct Downloader<T> {
    operator: T,
    body: Response,
    config: DownloadConfig,
    info: Option<DownloadInfo>,
}

impl Downloader<()> {
    /// Creates a `Downloader` that based on a default `DownloadOperator` which
    /// show progress on console.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use ylong_http_client::async_impl::{Downloader, HttpBody, Response};
    ///
    /// # async fn download_and_show_progress_on_console(response: Response) {
    /// // Creates a default `Downloader` that show progress on console.
    /// let mut downloader = Downloader::console(response);
    /// let _ = downloader.download().await;
    /// # }
    /// ```
    pub fn console(response: Response) -> Downloader<Console> {
        Self::builder().body(response).console().build()
    }

    /// Creates a `DownloaderBuilder` and configures downloader step by step.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::Downloader;
    ///
    /// let builder = Downloader::builder();
    /// ```
    pub fn builder() -> DownloaderBuilder<WantsBody> {
        DownloaderBuilder::new()
    }
}

impl<T: DownloadOperator + Unpin> Downloader<T> {
    /// Starts downloading that uses this `Downloader`'s configurations.
    ///
    /// The download and progress methods of the `DownloadOperator` will be
    /// called multiple times until the download is complete.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::{Downloader, HttpBody, Response};
    ///
    /// # async fn download_response_body(response: Response) {
    /// let mut downloader = Downloader::console(response);
    /// let _result = downloader.download().await;
    /// # }
    /// ```
    pub async fn download(&mut self) -> Result<(), HttpClientError> {
        // Construct new download info, or reuse previous info.
        if self.info.is_none() {
            let content_length = self
                .body
                .headers()
                .get("Content")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            self.info = Some(DownloadInfo::new(content_length));
        }
        self.limited_download().await
    }

    // Downloads response body with speed limitation.
    // TODO: Speed Limit.
    async fn limited_download(&mut self) -> Result<(), HttpClientError> {
        self.show_progress().await?;
        self.check_timeout()?;

        let mut buf = [0; 16 * 1024];

        loop {
            let data_size = match self.body.body_mut().data(&mut buf).await {
                Ok(0) => {
                    self.show_progress().await?;
                    return Ok(());
                }
                Ok(size) => size,
                Err(e) => {
                    return Err(HttpClientError::new_with_cause(
                        ErrorKind::BodyTransfer,
                        Some(e),
                    ))
                }
            };

            let data = &buf[..data_size];
            let mut size = 0;
            while size != data.len() {
                self.check_timeout()?;
                size += self.operator.download(&data[size..]).await?;
                self.info.as_mut().unwrap().downloaded_bytes += data.len() as u64;
                self.show_progress().await?;
            }
        }
    }

    fn check_timeout(&mut self) -> Result<(), HttpClientError> {
        if let Some(timeout) = self.config.timeout.inner() {
            let now = Instant::now();
            if now.duration_since(self.info.as_mut().unwrap().start_time) >= timeout {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Timeout,
                    "Download timeout",
                ));
            }
        }
        Ok(())
    }

    async fn show_progress(&mut self) -> Result<(), HttpClientError> {
        let info = self.info.as_mut().unwrap();
        self.operator
            .progress(info.downloaded_bytes, info.total_bytes)
            .await
    }
}

struct DownloadInfo {
    pub(crate) start_time: Instant,
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: Option<u64>,
}

impl DownloadInfo {
    fn new(total_bytes: Option<u64>) -> Self {
        Self {
            start_time: Instant::now(),
            downloaded_bytes: 0,
            total_bytes,
        }
    }
}

struct DownloadConfig {
    pub(crate) timeout: Timeout,
    pub(crate) speed_limit: SpeedLimit,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            timeout: Timeout::none(),
            speed_limit: SpeedLimit::none(),
        }
    }
}
