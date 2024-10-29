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

use core::ops::{Deref, DerefMut};

use ylong_http::body::async_impl::Body;
use ylong_http::response::Response as Resp;

use crate::async_impl::HttpBody;
use crate::error::HttpClientError;
use crate::util::monitor::TimeGroup;
use crate::ErrorKind;

/// A structure that represents an HTTP `Response`.
pub struct Response {
    pub(crate) inner: Resp<HttpBody>,
    pub(crate) time_group: TimeGroup,
}

impl Response {
    pub(crate) fn new(response: Resp<HttpBody>) -> Self {
        Self {
            inner: response,
            time_group: TimeGroup::default(),
        }
    }

    /// Reads the data of the `HttpBody`.
    pub async fn data(&mut self, buf: &mut [u8]) -> Result<usize, HttpClientError> {
        Body::data(self.inner.body_mut(), buf).await
    }

    /// Reads all the message of the `HttpBody` and return it as a `String`.
    pub async fn text(mut self) -> Result<String, HttpClientError> {
        let mut buf = [0u8; 1024];
        let mut vec = Vec::new();
        loop {
            let size = self.data(&mut buf).await?;
            if size == 0 {
                break;
            }
            vec.extend_from_slice(&buf[..size]);
        }

        String::from_utf8(vec).map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))
    }

    /// Gets the time spent on each stage of the request.
    pub fn time_group(&self) -> &TimeGroup {
        &self.time_group
    }

    pub(crate) fn set_time_group(&mut self, time_group: TimeGroup) {
        self.time_group = time_group
    }
}

impl Deref for Response {
    type Target = Resp<HttpBody>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Response {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
