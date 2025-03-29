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
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::async_impl::dns::resolver::{DefaultDnsFuture, DnsManager, DnsResult, ResolvedAddrs};
use crate::async_impl::{Body, Client, Request, Resolver, SocketFuture};
use crate::HttpClientError;

const DEFAULT_MAX_RETRY_COUNT: i32 = 1;

/// Doh resolver used by the `Client`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::async_impl::{Client, DohResolver};
///
/// let doh_resolver = DohResolver::new("https://1.12.12.12/dns-query");
/// let _doh_client = Client::builder()
///     .dns_resolver(doh_resolver)
///     .build()
///     .unwrap();
/// ```
pub struct DohResolver {
    manager: Option<DnsManager>,
    connector: DohConnector,
}

impl DohResolver {
    /// Creates a new DohResolver. And sets DOH server.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::DohResolver;
    ///
    /// let res = DohResolver::new("https://1.12.12.12/dns-query");
    /// ```
    pub fn new(doh_server: &str) -> Self {
        Self {
            manager: Some(DnsManager::default()),
            connector: DohConnector::new(doh_server),
        }
    }

    /// Adds the doh server.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::DohResolver;
    ///
    /// let res = DohResolver::new("https://1.12.12.12/dns-query")
    ///     .add_doh_server("https://1.12.12.12/dns-query");
    /// ```
    pub fn add_doh_server(mut self, doh_server: &str) -> Self {
        self.connector.add_doh_server(doh_server);
        self
    }

    /// Sets whether to use global DNS cache, default is false.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::async_impl::DohResolver;
    ///
    /// let res = DohResolver::new("https://1.12.12.12/dns-query").global_dns_cache(false);
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
    /// use ylong_http_client::async_impl::DohResolver;
    ///
    /// let res = DohResolver::new("https://1.12.12.12/dns-query").set_ttl(Duration::from_secs(30));
    /// ```
    pub fn set_ttl(mut self, ttl: Duration) -> Self {
        if let Some(manager) = self.manager.as_mut() {
            manager.ttl = ttl
        }
        self
    }
}

#[derive(Clone)]
struct DohConnector {
    doh_servers: Vec<String>,
    max_retry_count: i32,
}

impl DohConnector {
    fn new(doh_server: &str) -> Self {
        DohConnector {
            doh_servers: vec![doh_server.to_string()],
            max_retry_count: DEFAULT_MAX_RETRY_COUNT,
        }
    }

    fn add_doh_server(&mut self, doh_server: &str) {
        self.doh_servers.push(doh_server.to_string());
    }

    async fn retry(&self, authority: &str) -> Result<(Vec<SocketAddr>, u64), HttpClientError> {
        for _ in 0..self.max_retry_count {
            for server in self.doh_servers.iter() {
                if let Ok((socket_addr, ttl)) = self.doh_connect(authority, server.clone()).await {
                    return Ok((socket_addr, ttl));
                }
            }
        }
        Err(HttpClientError::from_str(
            crate::ErrorKind::Connect,
            "Can't find valid address",
        ))
    }

    /// Connects to the DOH server and retrieves DNS information.
    async fn doh_connect(
        &self,
        authority: &str,
        doh_server: String,
    ) -> Result<(Vec<SocketAddr>, u64), HttpClientError> {
        let part: Vec<&str> = authority.split(':').collect();
        let host: &str = part[0];
        let port: u16 = part[1].parse().unwrap();
        let url_4 = format!("{}?name={}&type=A", doh_server, host);
        let url_6 = format!("{}?name={}&type=AAAA", doh_server, host);
        let client_4 = Client::builder().build()?;
        let client_6 = Client::builder().build()?;
        let request_4 = Request::builder().url(&url_4).body(Body::empty())?;
        let request_6 = Request::builder().url(&url_6).body(Body::empty())?;
        let response_4 = client_4.request(request_4).await?;
        let response_6 = client_6.request(request_6).await?;
        let text_4 = response_4.text().await?;
        let text_6 = response_6.text().await?;
        let text = format!("{},{}", text_4, text_6);
        Ok(Self::get_info(&text, port))
    }

    /// Parses and extracts information from the DNS response text.
    fn get_info(text: &str, port: u16) -> (Vec<SocketAddr>, u64) {
        let mut ips = Vec::new();
        let mut start = 0;
        let mut ttl = u64::MAX;
        while let Some((answer_end, answer_str)) = Self::get_answer_str(text, start) {
            if let Some(socket_addr) = Self::get_socket_addr(answer_str, port) {
                if let Some(answer_ttl) = Self::get_ttl(answer_str) {
                    ips.push(socket_addr);
                    ttl = std::cmp::min(ttl, answer_ttl);
                }
            }
            start = answer_end + 1;
        }
        ttl = if ttl == u64::MAX { 0 } else { ttl };
        (ips, ttl)
    }

    fn get_answer_str(answer_section: &str, start: usize) -> Option<(usize, &str)> {
        let answer_start = answer_section[start..].find('{').map(|pos| start + pos)?;
        let answer_end = answer_section[answer_start..].find('}').unwrap() + answer_start;
        Some((answer_end, &answer_section[answer_start..answer_end]))
    }

    fn get_socket_addr(answer_str: &str, port: u16) -> Option<SocketAddr> {
        let data_str = r#""data":""#;
        if let Some(ip_pos) = answer_str.find(data_str) {
            let ip_start = ip_pos + data_str.len();
            if let Some(ip_end) = answer_str[ip_start..].find('\"') {
                let ip = &answer_str[ip_start..ip_start + ip_end];
                if let Ok(ipv4_addr) = Ipv4Addr::from_str(ip) {
                    return Some(SocketAddr::new(IpAddr::V4(ipv4_addr), port));
                }
                if let Ok(ipv6_addr) = Ipv6Addr::from_str(ip) {
                    return Some(SocketAddr::new(IpAddr::V6(ipv6_addr), port));
                }
            }
        }
        None
    }

    fn get_ttl(answer_str: &str) -> Option<u64> {
        let ttl_str = r#""TTL":"#;
        if let Some(ttl_pos) = answer_str.find(ttl_str) {
            let ttl_start = ttl_pos + ttl_str.len();
            if let Some(ttl_end) = answer_str[ttl_start..].find(',') {
                let ttl: u64 = answer_str[ttl_start..ttl_start + ttl_end].parse().unwrap();
                return Some(ttl);
            }
        }
        None
    }
}

impl Resolver for DohResolver {
    fn resolve(&self, authority: &str) -> SocketFuture {
        let authority = authority.to_string();
        let map = match &self.manager {
            None => {
                let manager = DnsManager::global_dns_manager();
                let manager_guard = manager.lock().unwrap();
                manager_guard.clean_expired_entries();
                manager_guard.map.clone()
            }
            Some(manager) => {
                manager.clean_expired_entries();
                manager.map.clone()
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
            #[cfg(feature = "ylong_base")]
            let result = ylong_runtime::block_on(connector.retry(&authority));
            #[cfg(feature = "tokio_base")]
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(connector.retry(&authority));
            match result {
                Ok((addrs, ttl)) => {
                    let dns_result =
                        DnsResult::new(addrs.clone(), Instant::now() + Duration::from_secs(ttl));
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
mod ut_doh_test {
    use super::*;

    /// UT test case for `DohResolver::global_dns_cache`
    ///
    /// # Brief
    /// 1. Creates a new `DohResolver` instance.
    /// 2. Verifies the default `manager` is None.
    /// 3. Calls `global_dns_cache` and check manager.
    #[test]
    fn ut_dns_resolver_global() {
        let mut resolver = DohResolver::new("https://1.12.12.12/dns-query");
        assert!(resolver.manager.is_some());
        resolver = resolver.global_dns_cache(true);
        assert!(resolver.manager.is_none());
        resolver = resolver.global_dns_cache(false);
        assert!(resolver.manager.is_some());
    }

    /// UT test case for `DohResolver::set_ttl()`
    ///
    /// # Brief
    /// 1. Creates a new `DohResolver` instance.
    /// 2. Verifies the default `ttl` is 60 second.
    /// 3. Calls `set_ttl` and check ttl.
    #[test]
    fn ut_dns_resolver_ttl() {
        let mut resolver = DohResolver::new("https://1.12.12.12/dns-query");
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

    /// UT test case for `get_info` function with IPv4 address
    ///
    /// # Brief
    /// 1. Provides a DNS response text for an IPv4 address.
    /// 2. Calls `get_info` to extract addresses and TTL.
    /// 3. Verifies the extracted address and TTL.
    #[test]
    fn ut_get_info_ipv4() {
        let ipv4_text = r#"{"Status":0,"TC":false,"RD":true,"RA":true,"AD":false,"CD":false,"Question":[{"name":"example.com.","type":1}],"Answer":[{"name":"example.com.","type":1,"TTL":3378,"data":"93.184.215.14"}]}"#;
        let (addrs, ttl) = DohConnector::get_info(ipv4_text, 0);
        assert_eq!(addrs, vec![SocketAddr::from(([93, 184, 215, 14], 0))]);
        assert_eq!(ttl, 3378);
    }

    /// UT test case for `get_info` function with IPv6 address
    ///
    /// # Brief
    /// 1. Provides a DNS response text for an IPv6 address.
    /// 2. Calls `get_info` to extract addresses and TTL.
    /// 3. Verifies the extracted address and TTL.
    #[test]
    fn ut_get_info_ipv6() {
        let ipv6_text = r#"{"Status":0,"TC":false,"RD":true,"RA":true,"AD":false,"CD":false,"Question":[{"name":"example.com.","type":28}]"Answer":[{"name":example.com.","type":28,"TTL":1466,"data":"2606:2800:21f:cb07:6820:80da:af6b:8b2c"}]}"#;
        let (addrs, ttl) = DohConnector::get_info(ipv6_text, 0);
        assert_eq!(
            addrs,
            vec![SocketAddr::from((
                [0x2606, 0x2800, 0x21f, 0xcb07, 0x6820, 0x80da, 0xaf6b, 0x8b2c],
                0
            ))]
        );
        assert_eq!(ttl, 1466);
    }

    /// UT test case for `get_info` function with both IPv4 and IPv6 addresses
    ///
    /// # Brief
    /// 1. Provides a DNS response text with both IPv4 and IPv6 addresses.
    /// 2. Calls `get_info` to extract the addresses and TTL.
    /// 3. Verifies the extracted addresses and TTL.
    #[test]
    fn ut_get_info_both() {
        let text = r#"{"Status":0,"TC":false,"RD":true,"RA":true,"AD":false,"CD":false,"Question":[{"name":"example.com.","type":1}],"Answer":[{"name":"example.com.","type":1,"TTL":3378,"data":"93.184.215.14"}]},{"Status":0,"TC":false,"RD":true,"RA":true,"AD":false,"CD":false,"Question":[{"name":"example.com.","type":28}],"Answer":[{"name":"example.com.","type":28,"TTL":1466,"data":"2606:2800:21f:cb07:6820:80da:af6b:8b2c"}]}"#;
        let (addrs, ttl) = DohConnector::get_info(text, 0);
        assert_eq!(
            addrs,
            vec![
                SocketAddr::from(([93, 184, 215, 14], 0)),
                SocketAddr::from((
                    [0x2606, 0x2800, 0x21f, 0xcb07, 0x6820, 0x80da, 0xaf6b, 0x8b2c],
                    0
                )),
            ]
        );
        assert_eq!(ttl, 1466);
    }

    /// UT test case for `get_info` function with some error response.
    ///
    /// # Brief
    /// 1. Provides a DNS response text with some error response.
    /// 2. Calls `get_info` to extract the addresses and TTL.
    /// 3. Verifies addresses is empty and TTL is the max of u64.
    #[test]
    fn ut_get_info_error() {
        let error_text = "This is some error response.";
        let (addrs, ttl) = DohConnector::get_info(error_text, 0);
        assert_eq!(addrs, vec![]);
        assert_eq!(ttl, 0);
    }
}
