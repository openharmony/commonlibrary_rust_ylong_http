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

use core::cmp;
use core::time::Duration;

use crate::error::HttpClientError;
use crate::util::{proxy, redirect};

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
/// // No redirect.
/// let no_redirect = Redirect::none();
///
/// // Custom the number of redirects.
/// let max = Redirect::limited(10);
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Redirect(redirect::Redirect);

impl Redirect {
    /// Sets max number of redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::Redirect;
    ///
    /// let redirect = Redirect::limited(10);
    /// ```
    pub fn limited(max: usize) -> Self {
        Self(redirect::Redirect::limited(max))
    }

    /// Sets unlimited number of redirects.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::Redirect;
    ///
    /// let redirect = Redirect::no_limit();
    /// ```
    pub fn no_limit() -> Self {
        Self(redirect::Redirect::limited(usize::MAX))
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
        Self(redirect::Redirect::none())
    }

    pub(crate) fn inner(&self) -> &redirect::Redirect {
        &self.0
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

    /// Customizes the number of retries. Returns `Err` if `times` is greater
    /// than 3.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::Retry;
    ///
    /// assert!(Retry::new(1).is_ok());
    /// assert!(Retry::new(10).is_err());
    /// ```
    pub fn new(times: usize) -> Result<Self, HttpClientError> {
        if times >= Self::MAX_RETRIES {
            return err_from_msg!(Build, "Invalid params");
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
    /// use ylong_http_client::Retry;
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
    /// The unit of speed is bytes per second, and the unit of duration is
    /// seconds.
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
    /// Pass HTTPS to the proxy URL, but the https uri which is in the no proxy
    /// list, will not pass the proxy URL.
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

    /// Pass HTTPS to the proxy URL, and set username and password which is
    /// required by the proxy server.
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
    use std::time::Duration;

    use ylong_http::request::uri::Uri;

    use crate::{Proxy, Redirect, Retry, SpeedLimit, Timeout};

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

    /// UT test cases for `Redirect::default`.
    ///
    /// # Brief
    /// 1. Creates a `Redirect` by calling `Redirect::default`.
    /// 2. Creates a 10 Redirect.
    /// 3. Checks if the results are correct.
    #[test]
    #[allow(clippy::redundant_clone)]
    fn ut_redirect_clone() {
        let redirect = Redirect::default();
        let redirect_10 = Redirect::limited(10);
        assert_eq!(redirect, redirect_10);
        assert_eq!(redirect.clone(), redirect_10)
    }

    /// UT test cases for `Retry::clone`.
    ///
    /// # Brief
    /// 1. Creates a `Retry` by calling `Redirect::new`.
    /// 2. Creates another `Retry` by `Redirect::clone`.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_retry_clone() {
        let retry = Retry::new(1).unwrap();
        assert_eq!(retry.clone(), retry)
    }

    /// UT test cases for `Retry::default`.
    ///
    /// # Brief
    /// 1. Creates a `Retry` by calling `Redirect::default`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_retry_default() {
        let retry = Retry::default();
        assert_eq!(retry, Retry::none())
    }

    /// UT test cases for `Retry::max`.
    ///
    /// # Brief
    /// 1. Creates a `Retry` by calling `Redirect::max`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_retry_max() {
        let retry = Retry::max();
        assert_eq!(retry.times(), Some(3))
    }

    /// UT test cases for `Timeout::clone`.
    ///
    /// # Brief
    /// 1. Creates a `Timeout` by calling `Timeout::from_secs`.
    /// 2. Creates another `Timeout` by `Timeout::clone`.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_timeout_clone() {
        let timeout = Timeout::from_secs(5);
        assert_eq!(timeout.clone(), timeout)
    }

    /// UT test cases for `Timeout::default`.
    ///
    /// # Brief
    /// 1. Creates a `Timeout` by calling `Timeout::default`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_timeout_default() {
        let timeout = Timeout::default();
        assert_eq!(timeout, Timeout::none())
    }

    /// UT test cases for `SpeedLimit::default`.
    ///
    /// # Brief
    /// 1. Creates a `SpeedLimit` by calling `SpeedLimit::default`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_speed_limit_default() {
        let speed = SpeedLimit::new();
        assert_eq!(speed.max, SpeedLimit::default().max)
    }

    /// UT test cases for `SpeedLimit::min_speed`.
    ///
    /// # Brief
    /// 1. Creates a `SpeedLimit` by calling `SpeedLimit::new`.
    /// 2. Sets the max speed of `SpeedLimit`.
    /// 3. Sets a min speed value that is greater than the max speed value.
    /// 4. Checks if the results are correct.
    #[test]
    fn ut_speed_limit_min_speed() {
        let speed = SpeedLimit::new();
        let speed = speed.max_speed(1024);
        let speed = speed.min_speed(2048, 12);
        assert_eq!(speed.min, (1024, Duration::from_secs(12)))
    }

    /// UT test cases for `Proxy::clone`.
    ///
    /// # Brief
    /// 1. Creates a `Proxy` by calling `Proxy::all`.
    /// 2. Creates another `Proxy` by `Timeout::clone`.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_proxy_clone() {
        let proxy = Proxy::all("http://127.0.0.1:6789")
            .no_proxy("127.0.0.1")
            .basic_auth("user", "password")
            .build()
            .unwrap();
        let proxy_clone = proxy.clone();
        let uri = Uri::from_bytes(b"http://127.0.0.1:3456").unwrap();
        assert!(!proxy.inner().is_intercepted(&uri));
        assert!(!proxy_clone.inner().is_intercepted(&uri));
    }

    /// UT test cases for `Proxy::https`.
    ///
    /// # Brief
    /// 1. Creates a `Proxy` by calling `Proxy::https`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_proxy_https() {
        let proxy = Proxy::https("http://127.0.0.1:6789").build().unwrap();
        let uri = Uri::from_bytes(b"https://127.0.0.1:3456").unwrap();
        assert!(proxy.inner().is_intercepted(&uri));
    }
}
