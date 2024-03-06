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

/// A `UploadOperator` represents structures that can read local data to socket.
///
/// You can implement `UploadOperator` for your structures and pass it to a
/// `Uploader`. Then the `Uploader` can use the `progress` methods to help you
/// upload the local data.
///
/// # Examples
///
/// ```
/// # use std::pin::Pin;
/// # use std::task::{Context, Poll};
/// # use ylong_http_client::async_impl::UploadOperator;
/// # use ylong_http_client::HttpClientError;
///
/// // Creates your own operator.
/// struct MyUploadOperator;
///
/// // Implements `DownloaderOperator` for your structures.
/// impl UploadOperator for MyUploadOperator {
///     fn poll_progress(
///         self: Pin<&mut Self>,
///         cx: &mut Context<'_>,
///         uploaded: u64,
///         total: Option<u64>,
///     ) -> Poll<Result<(), HttpClientError>> {
///         todo!()
///     }
/// }
/// ```
pub trait UploadOperator {
    /// The progress method that you need to implement. You need to perform some
    /// operations in this method based on the number of bytes uploaded and
    /// the total size.
    fn poll_progress(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _uploaded: u64,
        _total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        Poll::Ready(Ok(()))
    }

    /// Creates a `Progress` Future.
    fn progress<'a>(&'a mut self, uploaded: u64, total: Option<u64>) -> ProgressFuture<'a, Self>
    where
        Self: Unpin + Sized + 'a,
    {
        ProgressFuture {
            operator: self,
            uploaded,
            total,
        }
    }
}

impl<T> UploadOperator for &mut T
where
    T: UploadOperator + Unpin,
{
    fn poll_progress(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        uploaded: u64,
        total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        Pin::new(&mut **self).poll_progress(cx, uploaded, total)
    }
}

/// A future that execute `poll_progress` method.
pub struct ProgressFuture<'a, T> {
    operator: &'a mut T,
    uploaded: u64,
    total: Option<u64>,
}

impl<'a, T> Future for ProgressFuture<'a, T>
where
    T: UploadOperator + Unpin + 'a,
{
    type Output = Result<(), HttpClientError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        Pin::new(&mut fut.operator).poll_progress(cx, fut.uploaded, fut.total)
    }
}

/// A default download operator that display messages on console.
pub struct Console;

impl UploadOperator for Console {
    fn poll_progress(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        uploaded: u64,
        _total: Option<u64>,
    ) -> Poll<Result<(), HttpClientError>> {
        println!("progress: upload-{uploaded} bytes");
        Poll::Ready(Ok(()))
    }
}
