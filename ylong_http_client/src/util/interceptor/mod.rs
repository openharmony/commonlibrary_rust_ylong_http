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

//! Http network interceptor.

use ylong_http::response::Response as HttpResp;

use crate::async_impl::{HttpBody, Request, Response};
use crate::{ConnDetail, HttpClientError};

pub(crate) type Interceptors = dyn Interceptor + Sync + Send + 'static;

/// Transport layer protocol type.
#[derive(Clone)]
pub enum ConnProtocol {
    /// Tcp protocol.
    Tcp,
    /// Udp Protocol.
    Udp,
    /// Quic Protocol
    Quic,
}

/// Network interceptor.
///
/// Provides intercepting behavior at various stages of http message passing.
pub trait Interceptor {
    /// Intercepts the created transport layer protocol.
    // TODO add cache and response interceptor.
    // Is it necessary to add a response interceptor?
    // Does the input and output interceptor need to be added to http2 or http3
    // encoded packets?
    fn intercept_connection(&self, _info: ConnDetail) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the input of transport layer io.
    fn intercept_input(&self, _bytes: &[u8]) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the output of transport layer io.
    fn intercept_output(&self, _bytes: &[u8]) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the Request that is eventually transmitted to the peer end.
    fn intercept_request(&self, _request: &Request) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the response that is eventually returned.
    fn intercept_response(&self, _response: &Response) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the error cause of the retry.
    fn intercept_retry(&self, _error: &HttpClientError) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the redirect request.
    fn intercept_redirect_request(&self, _request: &Request) -> Result<(), HttpClientError> {
        Ok(())
    }

    /// Intercepts the response returned by the redirect
    fn intercept_redirect_response(
        &self,
        _response: &HttpResp<HttpBody>,
    ) -> Result<(), HttpClientError> {
        Ok(())
    }
}

/// The default Interceptor does not do any intercepting.
pub(crate) struct IdleInterceptor;

impl Interceptor for IdleInterceptor {}
