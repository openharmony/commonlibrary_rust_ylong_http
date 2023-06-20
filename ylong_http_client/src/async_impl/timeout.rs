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

use crate::async_impl::HttpBody;
use crate::Sleep;
use crate::{ErrorKind, HttpClientError};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use ylong_http::response::Response;

pub(crate) struct TimeoutFuture<T> {
    pub(crate) timeout: Option<Pin<Box<Sleep>>>,
    pub(crate) future: T,
}

impl<T> Future for TimeoutFuture<T>
where
    T: Future<Output = Result<Response<HttpBody>, HttpClientError>> + Unpin,
{
    type Output = Result<Response<HttpBody>, HttpClientError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if let Some(delay) = this.timeout.as_mut() {
            if let Poll::Ready(()) = delay.as_mut().poll(cx) {
                return Poll::Ready(Err(HttpClientError::new_with_message(
                    ErrorKind::Timeout,
                    "Request timeout",
                )));
            }
        }
        match Pin::new(&mut this.future).poll(cx) {
            Poll::Ready(Ok(mut response)) => {
                response.body_mut().set_sleep(this.timeout.take());
                Poll::Ready(Ok(response))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}
