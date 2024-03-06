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

use std::io::{Read, Write};

use super::{Body, Request, Response};
use crate::error::HttpClientError;
use crate::sync_impl::HttpBody;
use crate::util::dispatcher::Conn;

#[cfg(feature = "http1_1")]
mod http1;

pub(crate) trait StreamData: Read {
    fn shutdown(&self);
}

pub(crate) fn request<S, T>(
    conn: Conn<S>,
    request: &mut Request<T>,
) -> Result<Response<HttpBody>, HttpClientError>
where
    T: Body,
    S: Read + Write + 'static,
{
    match conn {
        #[cfg(feature = "http1_1")]
        Conn::Http1(http1) => http1::request(http1, request),

        #[cfg(feature = "http2")]
        Conn::Http2(_) => todo!(),
    }
}
