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

use crate::error::ErrorKind;
use crate::error::HttpClientError;
use crate::util;
use ylong_http::headers::Headers;
use ylong_http::request::method::Method;
use ylong_http::request::uri::Uri;
use ylong_http::request::Request;
use ylong_http::response::status::StatusCode;
use ylong_http::response::Response;

/// Redirect strategy supports limited times of redirection and no redirect
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedirectStrategy {
    inner: StrategyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StrategyKind {
    LimitTimes(usize),
    NoRedirect,
}

/// Redirect status supports to check response status and next
/// redirected uri
#[derive(Clone)]
pub struct RedirectStatus<'a> {
    previous_uri: &'a [Uri],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Trigger {
    inner: TriggerKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TriggerKind {
    NextLink,
    Stop,
}

impl RedirectStrategy {
    pub(crate) fn limited(max: usize) -> Self {
        Self {
            inner: StrategyKind::LimitTimes(max),
        }
    }

    pub(crate) fn none() -> Self {
        Self {
            inner: StrategyKind::NoRedirect,
        }
    }

    pub(crate) fn redirect(&self, status: RedirectStatus) -> Result<Trigger, HttpClientError> {
        match self.inner {
            StrategyKind::LimitTimes(max) => {
                if status.previous_uri.len() >= max {
                    Err(HttpClientError::new_with_message(
                        ErrorKind::Build,
                        "Over redirect max limit",
                    ))
                } else {
                    Ok(status.transfer())
                }
            }
            StrategyKind::NoRedirect => Ok(status.stop()),
        }
    }

    pub(crate) fn get_trigger(
        &self,
        redirect_status: RedirectStatus,
    ) -> Result<TriggerKind, HttpClientError> {
        let trigger = self.redirect(redirect_status)?;
        Ok(trigger.inner)
    }
}

impl Default for RedirectStrategy {
    fn default() -> RedirectStrategy {
        RedirectStrategy::limited(10)
    }
}

impl<'a> RedirectStatus<'a> {
    pub(crate) fn new(previous_uri: &'a [Uri]) -> Self {
        Self { previous_uri }
    }

    fn transfer(self) -> Trigger {
        Trigger {
            inner: TriggerKind::NextLink,
        }
    }

    fn stop(self) -> Trigger {
        Trigger {
            inner: TriggerKind::Stop,
        }
    }
}

pub(crate) struct Redirect;

impl Redirect {
    pub(crate) fn get_trigger_kind<T, K>(
        dst_uri: &mut Uri,
        redirect: &util::Redirect,
        redirect_list: &[Uri],
        response: &Response<K>,
        request: &mut Request<T>,
    ) -> Result<TriggerKind, HttpClientError> {
        let location = match response.headers().get("location") {
            Some(value) => value,
            None => {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Redirect,
                    "No location in response's headers",
                ));
            }
        };

        let loc_str = location.to_str().unwrap();
        let loc_bytes = loc_str.as_str().trim_start_matches('/').as_bytes();

        let mut loc_uri = Uri::from_bytes(loc_bytes)
            .map_err(|e| HttpClientError::new_with_cause(ErrorKind::Redirect, Some(e)))?;
        if loc_uri.scheme().is_none() || loc_uri.authority().is_none() {
            // request uri is existed, so can use unwrap directly
            let origin_scheme = request
                .uri()
                .scheme()
                .ok_or_else(|| HttpClientError::new_with_message(
                    ErrorKind::Connect,
                    "No uri scheme in request",
                ))?
                .as_str();
            let auth = request
                .uri()
                .authority()
                .ok_or_else(|| HttpClientError::new_with_message(
                    ErrorKind::Connect,
                    "No uri authority in request",
                ))?
                .to_str();
            let origin_auth = auth.as_str();
            loc_uri = Uri::builder()
                .scheme(origin_scheme)
                .authority(origin_auth)
                // loc_uri is existed, so can use unwrap directly
                .path(
                    loc_uri
                        .path()
                        .ok_or_else(|| HttpClientError::new_with_message(
                            ErrorKind::Connect,
                            "No loc_uri path in location",
                        ))?
                        .as_str(),
                )
                .query(
                    loc_uri
                        .query()
                        .ok_or_else(|| HttpClientError::new_with_message(
                            ErrorKind::Connect,
                            "No loc_uri query in location",
                        ))?
                        .as_str(),
                )
                .build()
                .unwrap();
        }

        let redirect_status = RedirectStatus::new(redirect_list);
        let trigger = redirect
            .redirect_strategy()
            .unwrap()
            .get_trigger(redirect_status)?;

        match trigger {
            TriggerKind::NextLink => {
                Self::remove_sensitive_headers(request.headers_mut(), &loc_uri, redirect_list);
                *dst_uri = loc_uri.clone();
                *request.uri_mut() = loc_uri;
                Ok(TriggerKind::NextLink)
            }
            TriggerKind::Stop => Ok(TriggerKind::Stop),
        }
    }

    fn remove_sensitive_headers(headers: &mut Headers, next: &Uri, previous: &[Uri]) {
        if let Some(previous) = previous.last() {
            // TODO: Check this logic.
            let cross_host = next.authority().unwrap() != previous.authority().unwrap();
            if cross_host {
                let _ = headers.remove("authorization");
                let _ = headers.remove("cookie");
                let _ = headers.remove("cookie2");
                let _ = headers.remove("proxy_authorization");
                let _ = headers.remove("www_authenticate");
            }
        }
    }

    pub(crate) fn check_redirect<T>(status_code: StatusCode, request: &mut Request<T>) -> bool {
        match status_code {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER => {
                Self::update_header_and_method(request);
                true
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => true,
            _ => false,
        }
    }

    fn update_header_and_method<T>(request: &mut Request<T>) {
        for header_name in [
            "transfer_encoding",
            "content_encoding",
            "content_type",
            "content_length",
            "content_language",
            "content_location",
            "digest",
            "last_modified",
        ] {
            let _ = request.headers_mut().remove(header_name);
        }
        let method = request.method_mut();
        match *method {
            Method::GET | Method::HEAD => {}
            _ => {
                *method = Method::GET;
            }
        }
    }
}

#[cfg(test)]
mod ut_redirect {
    use crate::redirect::Redirect;
    use crate::util::config::Redirect as setting_redirect;
    use crate::util::redirect::{RedirectStatus, RedirectStrategy, TriggerKind};
    use ylong_http::h1::ResponseDecoder;
    use ylong_http::request::uri::Uri;
    use ylong_http::request::Request;
    use ylong_http::response::status::StatusCode;
    use ylong_http::response::Response;
    /// UT test cases for `Redirect::check_redirect`.
    ///
    /// # Brief
    /// 1. Creates a `request` by calling `request::new`.
    /// 2. Uses `redirect::check_redirect` to check whether is redirected.
    /// 3. Checks if the result is true.
    #[test]
    fn ut_check_redirect() {
        let mut request = Request::new("this is a body");
        let code = StatusCode::MOVED_PERMANENTLY;
        let res = Redirect::check_redirect(code, &mut request);
        assert!(res);
    }
    /// UT test cases for `Redirect::get_trigger_kind`.
    ///
    /// # Brief
    /// 1. Creates a `redirect` by calling `setting_redirect::default`.
    /// 2. Uses `Redirect::get_trigger_kind` to get redirected trigger kind.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_get_trigger_kind() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nLocation: \t http://example3.com:80/foo?a=1 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response::from_raw_parts(result.0, result.1);
        let mut request = Request::new("this is a body");
        let request_uri = request.uri_mut();
        *request_uri = Uri::from_bytes(b"http://example1.com:80/foo?a=1").unwrap();
        let mut uri = Uri::default();
        let redirect = setting_redirect::default();
        let redirect_list: Vec<Uri> = vec![];
        let res = Redirect::get_trigger_kind(
            &mut uri,
            &redirect,
            &redirect_list,
            &response,
            &mut request,
        );
        assert!(res.is_ok());
    }
    /// UT test cases for `Redirect::get_trigger_kind` err branch.
    ///
    /// # Brief
    /// 1. Creates a `redirect` by calling `setting_redirect::default`.
    /// 2. Uses `Redirect::get_trigger_kind` to get redirected trigger kind.
    /// 3. Checks if the results are error.
    #[test]
    fn ut_get_trigger_kind_err() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nLocation: \t example3.com:80 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response::from_raw_parts(result.0, result.1);
        let mut request = Request::new("this is a body");
        let request_uri = request.uri_mut();
        *request_uri = Uri::from_bytes(b"http://example1.com:80").unwrap();
        let mut uri = Uri::default();
        let redirect = setting_redirect::default();
        let redirect_list: Vec<Uri> = vec![];
        let res = Redirect::get_trigger_kind(
            &mut uri,
            &redirect,
            &redirect_list,
            &response,
            &mut request,
        );
        assert!(res.is_err());
    }

    /// UT test cases for `RedirectStrategy::default`.
    ///
    /// # Brief
    /// 1. Creates a `RedirectStrategy` by calling `RedirectStrategy::default`.
    /// 2. Uses `RedirectStrategy::get_trigger` to get redirected uri.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_redirect_default() {
        let strategy = RedirectStrategy::default();
        let next = Uri::from_bytes(b"http://example.com").unwrap();
        let previous = (0..9)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        let redirect_uri = match strategy
            .get_trigger(RedirectStatus::new(&previous))
            .unwrap()
        {
            TriggerKind::NextLink => next.to_string(),
            TriggerKind::Stop => previous.get(9).unwrap().to_string(),
        };
        assert_eq!(redirect_uri, "http://example.com".to_string());
    }

    /// UT test cases for `RedirectStrategy::limited`.
    ///
    /// # Brief
    /// 1. Creates a `RedirectStrategy` by calling `RedirectStrategy::limited`.
    /// 2. Sets redirect times which is over max limitation times.
    /// 3. Uses `RedirectStrategy::get_trigger` to get redirected uri.
    /// 4. Checks if the results are err.
    #[test]
    fn ut_redirect_over_redirect_max() {
        let strategy = RedirectStrategy::limited(10);
        let previous = (0..10)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        if let Ok(other) = strategy.get_trigger(RedirectStatus::new(&previous)) {
            panic!("unexpected {:?}", other);
        }
    }

    /// UT test cases for `RedirectStrategy::none`.
    ///
    /// # Brief
    /// 1. Creates a `RedirectStrategy` by calling `RedirectStrategy::none`.
    /// 2. Uses `RedirectStrategy::get_trigger` but get origin uri.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_no_redirect() {
        let strategy = RedirectStrategy::none();
        let next = Uri::from_bytes(b"http://example.com").unwrap();
        let previous = (0..1)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        let redirect_uri = match strategy
            .get_trigger(RedirectStatus::new(&previous))
            .unwrap()
        {
            TriggerKind::NextLink => next.to_string(),
            TriggerKind::Stop => previous.get(0).unwrap().to_string(),
        };
        assert_eq!(redirect_uri, "http://example0.com".to_string());
    }
}
