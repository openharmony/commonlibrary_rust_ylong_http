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

use crate::error::{ErrorKind, HttpClientError};
use crate::util::proxy;
use crate::util::redirect as redirect_util;
use crate::util::redirect::{RedirectStrategy, TriggerKind};
use core::cmp;
use core::time::Duration;
use ylong_http::request::uri::Uri;
use ylong_http::request::Request;
use ylong_http::response::status::StatusCode;
use ylong_http::response::Response;

/// Redirects settings of requests.
///
/// # Example
///
/// ```
/// use ylong_http_client::Redirect;
///
/// // The default maximum number of redirects is 10.
/// let redirect = Redirect::default();
///
/// // No redirect
/// let no_redirect = Redirect::none();
///
/// // Custom the number of redirects.
/// let max = Redirect::limited(10);
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Redirect(Option<RedirectStrategy>);

impl Redirect {
    /// Gets the strategy of redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::util::Redirect;
    ///
    /// # let redirect = Redirect::limited(10);
    /// let strategy = redirect.redirect_strategy();
    ///
    /// # assert!(strategy.is_some());
    /// ```
    pub fn redirect_strategy(&self) -> Option<&redirect_util::RedirectStrategy> {
        self.0.as_ref()
    }

    /// Sets max number of redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::util::Redirect;
    ///
    /// let redirect = Redirect::limited(10);
    /// ```
    pub fn limited(max: usize) -> Self {
        Self(Some(RedirectStrategy::limited(max)))
    }

    /// Sets unlimited number of redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::util::Redirect;
    ///
    /// let redirect = Redirect::no_limit();
    /// ```
    pub fn no_limit() -> Self {
        Self(Some(RedirectStrategy::limited(usize::MAX)))
    }

    /// Stops redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::Redirect;
    ///
    /// let redirect = Redirect::none();
    /// ```
    pub fn none() -> Self {
        Self(Some(RedirectStrategy::none()))
    }

    pub(crate) fn get_redirect<T, K>(
        dst_uri: &mut Uri,
        redirect: &Redirect,
        redirect_list: &[Uri],
        response: &Response<K>,
        request: &mut Request<T>,
    ) -> Result<TriggerKind, HttpClientError> {
        redirect_util::Redirect::get_trigger_kind(
            dst_uri,
            redirect,
            redirect_list,
            response,
            request,
        )
    }

    pub(crate) fn is_redirect<T>(status_code: StatusCode, request: &mut Request<T>) -> bool {
        redirect_util::Redirect::check_redirect(status_code, request)
    }
}

impl Default for Redirect {
    // redirect default limit 10 times
    fn default() -> Self {
        Self(Some(RedirectStrategy::default()))
    }
}

/// Retries settings of requests. The default value is `Retry::NEVER`.
///
/// # Example
///
/// ```
/// use ylong_http_client::Retry;
///
/// // Never retry.
/// let never = Retry::none();
///
/// // The maximum number of redirects is 3.
/// let max = Retry::max();
///
/// // Custom the number of retries.
/// let custom = Retry::new(2).unwrap();
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Retry(Option<usize>);

impl Retry {
    const MAX_RETRIES: usize = 3;

    /// Customizes the number of retries. Returns `Err` if `times` is greater than 3.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::Retry;
    ///
    /// assert!(Retry::new(1).is_ok());
    /// assert!(Retry::new(10).is_err());
    /// ```
    pub fn new(times: usize) -> Result<Self, HttpClientError> {
        if times >= Self::MAX_RETRIES {
            return Err(HttpClientError::new_with_message(
                ErrorKind::Build,
                "Invalid Retry Times",
            ));
        }
        Ok(Self(Some(times)))
    }

    /// Creates a `Retry` that indicates never retry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Retry;
    ///
    /// let retry = Retry::none();
    /// ```
    pub fn none() -> Self {
        Self(None)
    }

    /// Creates a `Retry` with a max retry times.
    ///
    /// The maximum number of redirects is 3.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Retry;
    ///
    /// let retry = Retry::max();
    /// ```
    pub fn max() -> Self {
        Self(Some(Self::MAX_RETRIES))
    }

    /// Get the retry times, returns None if not set.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::util::Retry;
    ///
    /// assert!(Retry::default().times().is_none());
    /// ```
    pub fn times(&self) -> Option<usize> {
        self.0
    }
}

impl Default for Retry {
    fn default() -> Self {
        Self::none()
    }
}

/// Timeout settings.
///
/// # Examples
///
/// ```
/// use ylong_http_client::Timeout;
///
/// let timeout = Timeout::none();
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeout(Option<Duration>);

impl Timeout {
    /// Creates a `Timeout` without limiting the timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Timeout;
    ///
    /// let timeout = Timeout::none();
    /// ```
    pub fn none() -> Self {
        Self(None)
    }

    /// Creates a new `Timeout` from the specified number of whole seconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Timeout;
    ///
    /// let timeout = Timeout::from_secs(9);
    /// ```
    pub fn from_secs(secs: u64) -> Self {
        Self(Some(Duration::from_secs(secs)))
    }

    pub(crate) fn inner(&self) -> Option<Duration> {
        self.0
    }
}

impl Default for Timeout {
    fn default() -> Self {
        Self::none()
    }
}

/// Speed limit settings.
///
/// # Examples
///
/// ```
/// use ylong_http_client::SpeedLimit;
///
/// let limit = SpeedLimit::new();
/// ```
pub struct SpeedLimit {
    min: (u64, Duration),
    max: u64,
}

impl SpeedLimit {
    /// Creates a new `SpeedLimit`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::SpeedLimit;
    ///
    /// let limit = SpeedLimit::new();
    /// ```
    pub fn new() -> Self {
        Self::none()
    }

    /// Sets the minimum speed and the seconds for which the current speed is
    /// allowed to be less than this minimum speed.
    ///
    /// The unit of speed is bytes per second, and the unit of duration is seconds.
    ///
    /// The minimum speed cannot exceed the maximum speed that has been set. If
    /// the set value exceeds the currently set maximum speed, the minimum speed
    /// will be set to the current maximum speed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::SpeedLimit;
    ///
    /// // Sets minimum speed is 1024B/s, the duration is 10s.
    /// let limit = SpeedLimit::new().min_speed(1024, 10);
    /// ```
    pub fn min_speed(mut self, min: u64, secs: u64) -> Self {
        self.min = (cmp::min(self.max, min), Duration::from_secs(secs));
        self
    }

    /// Sets the maximum speed.
    ///
    /// The unit of speed is bytes per second.
    ///
    /// The maximum speed cannot be lower than the minimum speed that has been
    /// set. If the set value is lower than the currently set minimum speed, the
    /// maximum speed will be set to the current minimum speed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::SpeedLimit;
    ///
    /// let limit = SpeedLimit::new().max_speed(1024);
    /// ```
    pub fn max_speed(mut self, max: u64) -> Self {
        self.max = cmp::max(self.min.0, max);
        self
    }

    /// Creates a `SpeedLimit` without limiting the speed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::SpeedLimit;
    ///
    /// let limit = SpeedLimit::none();
    /// ```
    pub fn none() -> Self {
        Self {
            min: (0, Duration::MAX),
            max: u64::MAX,
        }
    }
}

impl Default for SpeedLimit {
    fn default() -> Self {
        Self::new()
    }
}

/// Proxy settings.
///
/// `Proxy` has functions which is below:
///
/// - replace origin uri by proxy uri to link proxy server.
/// - set username and password to login proxy server.
/// - set no proxy which can keep origin uri not to be replaced by proxy uri.
///
/// # Examples
///
/// ```
/// # use ylong_http_client::Proxy;
///
/// // All http request will be intercepted by `https://www.example.com`,
/// // but https request will link to server directly.
/// let proxy = Proxy::http("http://www.example.com").build();
///
/// // All https request will be intercepted by `http://www.example.com`,
/// // but http request will link to server directly.
/// let proxy = Proxy::https("http://www.example.com").build();
///
/// // All https and http request will be intercepted by "http://www.example.com".
/// let proxy = Proxy::all("http://www.example.com").build();
/// ```
#[derive(Clone)]
pub struct Proxy(proxy::Proxy);

impl Proxy {
    /// Passes all HTTP and HTTPS to the proxy URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// // All https and http request will be intercepted by `http://example.com`.
    /// let builder = Proxy::all("http://example.com");
    /// ```
    pub fn all(addr: &str) -> ProxyBuilder {
        ProxyBuilder {
            inner: proxy::Proxy::all(addr),
        }
    }

    /// Passes HTTP to the proxy URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// // All http request will be intercepted by https://example.com,
    /// // but https request will link to server directly.
    /// let proxy = Proxy::http("https://example.com");
    /// ```
    pub fn http(addr: &str) -> ProxyBuilder {
        ProxyBuilder {
            inner: proxy::Proxy::http(addr),
        }
    }

    /// Passes HTTPS to the proxy URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// // All https request will be intercepted by http://example.com,
    /// // but http request will link to server directly.
    /// let proxy = Proxy::https("http://example.com");
    /// ```
    pub fn https(addr: &str) -> ProxyBuilder {
        ProxyBuilder {
            inner: proxy::Proxy::https(addr),
        }
    }

    pub(crate) fn inner(self) -> proxy::Proxy {
        self.0
    }
}

/// A builder that constructs a `Proxy`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::Proxy;
///
/// let proxy = Proxy::all("http://www.example.com")
///     .basic_auth("Aladdin", "open sesame")
///     .build();
/// ```
pub struct ProxyBuilder {
    inner: Result<proxy::Proxy, HttpClientError>,
}

impl ProxyBuilder {
    /// Pass HTTPS to the proxy URL, but the https uri which is in the no proxy list, will not pass the proxy URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// let builder = Proxy::https("http://example.com").no_proxy("https://example2.com");
    /// ```
    pub fn no_proxy(mut self, no_proxy: &str) -> Self {
        self.inner = self.inner.map(|mut proxy| {
            proxy.no_proxy(no_proxy);
            proxy
        });
        self
    }

    /// Pass HTTPS to the proxy URL, and set username and password which is required by the proxy server.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// let builder = Proxy::https("http://example.com").basic_auth("username", "password");
    /// ```
    pub fn basic_auth(mut self, username: &str, password: &str) -> Self {
        self.inner = self.inner.map(|mut proxy| {
            proxy.basic_auth(username, password);
            proxy
        });
        self
    }

    /// Constructs a `Proxy`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Proxy;
    ///
    /// let proxy = Proxy::all("http://proxy.example.com").build();
    /// ```
    pub fn build(self) -> Result<Proxy, HttpClientError> {
        Ok(Proxy(self.inner?))
    }
}

#[cfg(test)]
mod ut_settings {
    use crate::error::HttpClientError;
    use crate::util::redirect as redirect_util;
    use crate::util::redirect::TriggerKind;
    use crate::{Redirect, Retry};
    use ylong_http::h1::ResponseDecoder;
    use ylong_http::request::uri::Uri;
    use ylong_http::request::Request;
    use ylong_http::response::status::StatusCode;
    use ylong_http::response::Response;

    fn create_trigger(
        redirect: &Redirect,
        previous: &[Uri],
    ) -> Result<redirect_util::TriggerKind, HttpClientError> {
        let redirect_status = redirect_util::RedirectStatus::new(previous);
        redirect.0.as_ref().unwrap().get_trigger(redirect_status)
    }
    /// UT test cases for `Redirect::is_redirect`.
    ///
    /// # Brief
    /// 1. Creates a `request` by calling `request::new`.
    /// 2. Uses `redirect::is_redirect` to check whether is redirected.
    /// 3. Checks if the result is true.
    #[test]
    fn ut_setting_is_redirect() {
        let mut request = Request::new("this is a body");
        let code = StatusCode::MOVED_PERMANENTLY;
        let res = Redirect::is_redirect(code, &mut request);
        assert!(res);
    }
    /// UT test cases for `Redirect::get_redirect` error branch.
    ///
    /// # Brief
    /// 1. Creates a `redirect` by calling `Redirect::default`.
    /// 2. Uses `Redirect::get_redirect` to get redirected trigger kind.
    /// 3. Checks if the results are error.
    #[test]
    fn ut_setting_get_redirect_kind_err() {
        let response_str = "HTTP/1.1 304 \r\nAge: \t 270646 \t \t\r\nDate: \t Mon, 19 Dec 2022 01:46:59 GMT \t \t\r\nEtag:\t \"3147526947+gzip\" \t \t\r\n\r\n".as_bytes();
        let mut decoder = ResponseDecoder::new();
        let result = decoder.decode(response_str).unwrap().unwrap();
        let response = Response::from_raw_parts(result.0, result.1);
        let mut request = Request::new("this is a body");
        let mut uri = Uri::default();
        let redirect = Redirect::default();
        let redirect_list: Vec<Uri> = vec![];
        let res =
            Redirect::get_redirect(&mut uri, &redirect, &redirect_list, &response, &mut request);
        assert!(res.is_err());
    }

    /// UT test cases for `Redirect::default`.
    ///
    /// # Brief
    /// 1. Creates a `Redirect` by calling `Redirect::default`.
    /// 2. Uses `Redirect::create_trigger` to get redirected uri.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_setting_redirect_default() {
        let redirect = Redirect::default();
        let next = Uri::from_bytes(b"http://example.com").unwrap();
        let previous = (0..9)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        let redirect_uri = match create_trigger(&redirect, &previous).unwrap() {
            TriggerKind::NextLink => next.to_string(),
            TriggerKind::Stop => previous.get(9).unwrap().to_string(),
        };
        assert_eq!(redirect_uri, "http://example.com".to_string());
    }

    /// UT test cases for `Redirect::max_limit`.
    ///
    /// # Brief
    /// 1. Creates a `Redirect` by calling `Redirect::max_limit`.
    /// 2. Sets redirect times which is over max limitation times.
    /// 2. Uses `Redirect::create_trigger` to get redirected uri.
    /// 3. Checks if the results are err.
    #[test]
    fn ut_setting_redirect_over_redirect_max() {
        let redirect = Redirect::limited(10);
        let previous = (0..10)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        if let Ok(other) = create_trigger(&redirect, &previous) {
            panic!("unexpected {:?}", other);
        };
    }

    /// UT test cases for `Redirect::no_redirect`.
    ///
    /// # Brief
    /// 1. Creates a `Redirect` by calling `Redirect::no_redirect`.
    /// 2. Uses `Redirect::create_trigger` but get origin uri.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_setting_no_redirect() {
        let redirect = Redirect::none();
        let next = Uri::from_bytes(b"http://example.com").unwrap();
        let previous = (0..1)
            .map(|i| Uri::from_bytes(format!("http://example{i}.com").as_bytes()).unwrap())
            .collect::<Vec<_>>();

        let redirect_uri = match create_trigger(&redirect, &previous).unwrap() {
            TriggerKind::NextLink => next.to_string(),
            TriggerKind::Stop => previous.get(0).unwrap().to_string(),
        };
        assert_eq!(redirect_uri, "http://example0.com".to_string());
    }

    /// UT test cases for `Retry::new`.
    ///
    /// # Brief
    /// 1. Creates a `Retry` by calling `Retry::new`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_retry_new() {
        let retry = Retry::new(1);
        assert!(retry.is_ok());
        let retry = Retry::new(3);
        assert!(retry.is_err());
        let retry = Retry::new(10);
        assert!(retry.is_err());
    }
}
