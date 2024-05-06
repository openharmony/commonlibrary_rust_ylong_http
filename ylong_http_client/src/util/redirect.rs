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

use ylong_http::request::method::Method;
use ylong_http::request::uri::Uri;
use ylong_http::request::Request;
use ylong_http::response::status::StatusCode;
use ylong_http::response::Response;

use crate::error::{ErrorKind, HttpClientError};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Redirect {
    strategy: Strategy,
}

impl Redirect {
    pub(crate) fn limited(times: usize) -> Self {
        Self {
            strategy: Strategy::LimitTimes(times),
        }
    }

    pub(crate) fn none() -> Self {
        Self {
            strategy: Strategy::NoRedirect,
        }
    }

    pub(crate) fn redirect<A, B>(
        &self,
        request: &mut Request<A>,
        response: &Response<B>,
        info: &mut RedirectInfo,
    ) -> Result<Trigger, HttpClientError> {
        match response.status() {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER => {
                for header_name in UPDATED_HEADERS {
                    let _ = request.headers_mut().remove(header_name);
                }
                let method = request.method_mut();
                match *method {
                    Method::GET | Method::HEAD => {}
                    _ => *method = Method::GET,
                }
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => {}
            _ => return Ok(Trigger::Stop),
        }

        info.previous.push(request.uri().clone());

        let mut location = response
            .headers()
            .get("Location")
            .and_then(|value| value.to_string().ok())
            .and_then(|str| Uri::try_from(str.as_bytes()).ok())
            .ok_or(HttpClientError::from_str(
                ErrorKind::Redirect,
                "Illegal location header in response",
            ))?;

        // If `location` doesn't have `scheme` or `authority`, adds scheme and
        // authority of the origin request to it.
        if location.scheme().is_none() || location.authority().is_none() {
            let origin = request.uri();
            let scheme = origin.scheme().cloned();
            let authority = origin.authority().cloned();
            let (_, _, path, query) = location.into_parts();
            location = Uri::from_raw_parts(scheme, authority, path, query);
        }

        let trigger = self.strategy.trigger(info)?;
        if let Trigger::NextLink = trigger {
            if let Some(previous) = info.previous.last() {
                if location.authority() != previous.authority() {
                    for header_name in SENSITIVE_HEADERS {
                        let _ = request.headers_mut().remove(header_name);
                    }
                }
            }
            *request.uri_mut() = location;
        }

        Ok(trigger)
    }
}

impl Default for Redirect {
    fn default() -> Self {
        Self::limited(10)
    }
}

pub(crate) struct RedirectInfo {
    previous: Vec<Uri>,
}

impl RedirectInfo {
    pub(crate) fn new() -> Self {
        Self {
            previous: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Strategy {
    LimitTimes(usize),
    NoRedirect,
}

impl Strategy {
    fn trigger(&self, info: &RedirectInfo) -> Result<Trigger, HttpClientError> {
        match self {
            Self::LimitTimes(max) => (info.previous.len() < *max)
                .then_some(Trigger::NextLink)
                .ok_or(HttpClientError::from_str(
                    ErrorKind::Build,
                    "Over redirect max limit",
                )),
            Self::NoRedirect => Ok(Trigger::Stop),
        }
    }
}

pub(crate) enum Trigger {
    NextLink,
    Stop,
}

const UPDATED_HEADERS: [&str; 8] = [
    "transfer-encoding",
    "content-encoding",
    "content-type",
    "content-length",
    "content-language",
    "content-location",
    "digest",
    "last-modified",
];

const SENSITIVE_HEADERS: [&str; 5] = [
    "authorization",
    "cookie",
    "cookie2",
    "proxy-authorization",
    "www-authenticate",
];
