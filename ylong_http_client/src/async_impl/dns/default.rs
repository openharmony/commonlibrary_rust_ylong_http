// Copyright (c) 2025 Huawei Device Co., Ltd.
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

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::async_impl::dns::resolver::{DefaultDnsFuture, DnsManager, DnsResult, ResolvedAddrs};
use crate::async_impl::{Resolver, SocketFuture};

/// Default dns resolver used by the `Client`.
/// DefaultDnsResolver provides DNS resolver with caching mechanism.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::{Client, DefaultDnsResolver};
///
/// let default_resolver = DefaultDnsResolver::new();
/// let _client = Client::builder()
///     .dns_resolver(default_resolver)
///     .build()
///     .unwrap();
/// ```
pub struct DefaultDnsResolver {
    /// Use global if None.
    manager: Option<DnsManager>,
    connector: DefaultDnsConnector,
}

impl Default for DefaultDnsResolver {
    // Default constructor for `DefaultDnsResolver`, with a default TTL of 60
    // seconds.
    fn default() -> Self {
        DefaultDnsResolver {
            manager: Some(DnsManager::default()),
            connector: DefaultDnsConnector {},
        }
    }
}

impl DefaultDnsResolver {
    /// Creates a new DefaultDnsResolver.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::DefaultDnsResolver;
    ///
    /// let res = DefaultDnsResolver::new();
    /// ```
    pub fn new() -> Self {
        DefaultDnsResolver {
            manager: Some(DnsManager::default()),
            connector: DefaultDnsConnector {},
        }
    }

    /// Sets whether to use global DNS cache, default is false.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::DefaultDnsResolver;
    ///
    /// let res = DefaultDnsResolver::new().global_dns_cache(true);
    /// ```
    pub fn global_dns_cache(mut self, use_global: bool) -> Self {
        self.manager = (!use_global).then(DnsManager::default);
        self
    }

    /// Sets DNS ttl, default is 60 second.
    ///
    /// This will does nothing if `global_dns_cache` is set to true.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use ylong_http_client::async_impl::DefaultDnsResolver;
    ///
    /// let res = DefaultDnsResolver::new().set_ttl(Duration::from_secs(30));
    /// ```
    pub fn set_ttl(mut self, ttl: Duration) -> Self {
        if let Some(manager) = self.manager.as_mut() {
            manager.ttl = ttl
        }
        self
    }
}

#[derive(Clone)]
struct DefaultDnsConnector {}

impl DefaultDnsConnector {
    // Resolves the authority to a list of socket addresses
    fn get_socket_addrs(&self, authority: &str) -> Result<Vec<SocketAddr>, io::Error> {
        authority
            .to_socket_addrs()
            .map(|addrs| addrs.collect())
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
    }
}

impl Resolver for DefaultDnsResolver {
    fn resolve(&self, authority: &str) -> SocketFuture {
        let authority = authority.to_string();
        let (map, ttl) = match &self.manager {
            None => {
                let manager = DnsManager::global_dns_manager();
                let manager_guard = manager.lock().unwrap();
                manager_guard.clean_expired_entries();
                (manager_guard.map.clone(), manager_guard.ttl)
            }
            Some(manager) => {
                manager.clean_expired_entries();
                (manager.map.clone(), manager.ttl)
            }
        };
        let connector = self.connector.clone();

        let handle = crate::runtime::spawn_blocking(move || {
            let mut map_lock = map.lock().unwrap();
            if let Some(addrs) = map_lock.get(&authority) {
                if addrs.is_valid() {
                    return Ok(ResolvedAddrs::new(addrs.addr.clone().into_iter()));
                }
            }
            match connector.get_socket_addrs(&authority) {
                Ok(addrs) => {
                    let dns_result = DnsResult::new(addrs.clone(), Instant::now() + ttl);
                    map_lock.insert(authority, dns_result);
                    Ok(ResolvedAddrs::new(addrs.into_iter()))
                }
                Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
            }
        });
        Box::pin(DefaultDnsFuture::new(handle))
    }
}

#[cfg(test)]
mod ut_dns_test {
    use super::*;

    /// UT test case for `DefaultDnsResolver::global_dns_cache()`
    ///
    /// # Brief
    /// 1. Creates a new `DefaultDnsResolver` instance.
    /// 2. Verifies the default `manager` is None.
    /// 3. Calls `global_dns_cache` and check manager.
    #[test]
    fn ut_dns_resolver_global() {
        let mut resolver = DefaultDnsResolver::new();
        assert!(resolver.manager.is_some());
        resolver = resolver.global_dns_cache(true);
        assert!(resolver.manager.is_none());
        resolver = resolver.global_dns_cache(false);
        assert!(resolver.manager.is_some());
    }

    /// UT test case for `DefaultDnsResolver::set_ttl()`
    ///
    /// # Brief
    /// 1. Creates a new `DefaultDnsResolver` instance.
    /// 2. Verifies the default `ttl` is 60 second.
    /// 3. Calls `set_ttl` and check ttl.
    #[test]
    fn ut_dns_resolver_ttl() {
        let mut resolver = DefaultDnsResolver::new();
        assert!(resolver.manager.is_some());
        assert_eq!(
            resolver.manager.as_ref().unwrap().ttl,
            Duration::from_secs(60)
        );
        resolver = resolver.set_ttl(Duration::from_secs(30));
        assert_eq!(
            resolver.manager.as_ref().unwrap().ttl,
            Duration::from_secs(30)
        );
    }

    /// UT test case for `DefaultDnsResolver::resolve`
    ///
    /// # Brief
    /// 1. Creates a default dns resolver with 50ms ttl.
    /// 2. Calls resolve to get socket address twice.
    /// 3. Verifies the second resolver is faster than the first one.
    /// 4. Verifies the second resolver result as same as the first one.
    #[tokio::test]
    #[cfg(feature = "tokio_base")]
    async fn ut_defualt_dns_resolver_resolve() {
        let authority = "example.com:0";
        let resolver = DefaultDnsResolver::new().set_ttl(Duration::from_secs(50));
        let start1 = Instant::now();
        let addrs1 = resolver.resolve(authority).await;
        let duration1 = start1.elapsed();
        assert!(addrs1.is_ok());
        tokio::time::sleep(Duration::from_millis(10)).await;
        let start2 = Instant::now();
        let addrs2 = resolver.resolve(authority).await;
        let duration2 = start2.elapsed();
        assert!(duration1 > duration2);
        assert!(addrs2.is_ok());
        if let (Ok(addr1), Ok(addr2)) = (addrs1, addrs2) {
            let vec1: Vec<SocketAddr> = addr1.collect();
            let vec2: Vec<SocketAddr> = addr2.collect();
            assert_eq!(vec1, vec2);
        }
    }
}
