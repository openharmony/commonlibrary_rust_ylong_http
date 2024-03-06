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

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::HttpClientError;

/// The trait defines the functionality required for processing bodies of HTTP
/// messages.
pub trait DownloadOperator {
    /// Attempts to write the body data read each time to the specified
    /// location.
    ///
    /// This method will be called every time a part of the body data is read.
    fn poll_download(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, HttpClientError>>;

    /// Attempts to inform you how many bytes have been written to
    /// the specified location at this time.
    ///
    /// You can display the progress according to the number of bytes written.
    fn poll_progress(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _downloaded: u64,
        _total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        Poll::Ready(Ok(()))
    }

    /// Returns future that writes the body data read each time to the
    /// specified location.
    ///
    /// This method will be called every time a part of the body data is read.
    fn download<'a, 'b>(&'a mut self, data: &'b [u8]) -> DownloadFuture<'a, 'b, Self>
    where
        Self: Unpin + Sized + 'a + 'b,
    {
        DownloadFuture {
            operator: self,
            data,
        }
    }

    /// Returns future that informs you how many bytes have been written to
    /// the specified location at this time.
    ///
    /// You can display the progress according to the number of bytes written.
    fn progress<'a>(&'a mut self, downloaded: u64, total: Option<u64>) -> ProgressFuture<'a, Self>
    where
        Self: Unpin + Sized + 'a,
    {
        ProgressFuture {
            processor: self,
            downloaded,
            total,
        }
    }
}

impl<T> DownloadOperator for &mut T
where
    T: DownloadOperator + Unpin,
{
    fn poll_download(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, HttpClientError>> {
        Pin::new(&mut **self).poll_download(cx, data)
    }

    fn poll_progress(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        downloaded: u64,
        total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        Pin::new(&mut **self).poll_progress(cx, downloaded, total)
    }
}

pub struct DownloadFuture<'a, 'b, T> {
    operator: &'a mut T,
    data: &'b [u8],
}

impl<'a, 'b, T> Future for DownloadFuture<'a, 'b, T>
where
    T: DownloadOperator + Unpin + 'a + 'b,
{
    type Output = Result<usize, HttpClientError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        Pin::new(&mut fut.operator).poll_download(cx, fut.data)
    }
}

pub struct ProgressFuture<'a, T> {
    processor: &'a mut T,
    downloaded: u64,
    total: Option<u64>,
}

impl<'a, T> Future for ProgressFuture<'a, T>
where
    T: DownloadOperator + Unpin + 'a,
{
    type Output = Result<(), HttpClientError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        Pin::new(&mut fut.processor).poll_progress(cx, fut.downloaded, fut.total)
    }
}

/// A default body processor that write data to console directly.
pub struct Console;

impl DownloadOperator for Console {
    fn poll_download(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, HttpClientError>> {
        println!(
            "{}",
            std::str::from_utf8(data).unwrap_or("<Contains non-UTF8>")
        );
        Poll::Ready(Ok(data.len()))
    }

    fn poll_progress(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        downloaded: u64,
        _total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        println!("progress: download-{downloaded} bytes");
        Poll::Ready(Ok(()))
    }
}
