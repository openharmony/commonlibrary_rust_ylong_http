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

//! Happy Eyeballs implementation.

use core::time::Duration;
use std::future::Future;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use crate::async_impl::dns::resolver::ResolvedAddrs;
use crate::runtime::{Sleep, TcpStream};

const HAPPY_EYEBALLS_PREFERRED_TIMEOUT_MS: u64 = 300;

pub(crate) struct HappyEyeballs {
    preferred_addr: RemoteAddrs,
    delay_addr: Option<DelayedAddrs>,
}

struct RemoteAddrs {
    addrs: DomainAddrs,
    // timeout of each socket address.
    timeout: Option<Duration>,
}

struct DelayedAddrs {
    addrs: RemoteAddrs,
    delay: Sleep,
}

pub(crate) struct EyeBallConfig {
    // the timeout period for the entire connection establishment.
    timeout: Option<Duration>,
    // Delay to start other address family when the preferred address family is not complete
    delay: Option<Duration>,
}

struct DomainAddrs {
    addrs: Vec<SocketAddr>,
}

struct DomainAddrsIter<'a> {
    iter: core::slice::Iter<'a, SocketAddr>,
}

impl DomainAddrs {
    pub(crate) fn new(addrs: Vec<SocketAddr>) -> Self {
        Self { addrs }
    }

    pub(crate) fn iter(&self) -> DomainAddrsIter {
        DomainAddrsIter {
            iter: self.addrs.iter(),
        }
    }
}

impl<'a> Iterator for DomainAddrsIter<'a> {
    type Item = &'a SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<'a> Deref for DomainAddrsIter<'a> {
    type Target = core::slice::Iter<'a, SocketAddr>;

    fn deref(&self) -> &Self::Target {
        &self.iter
    }
}

impl<'a> DerefMut for DomainAddrsIter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.iter
    }
}

impl DelayedAddrs {
    pub(crate) fn new(addrs: RemoteAddrs, delay: Sleep) -> Self {
        DelayedAddrs { addrs, delay }
    }
}

impl EyeBallConfig {
    pub(crate) fn new(timeout: Option<Duration>, delay: Option<Duration>) -> Self {
        Self { timeout, delay }
    }
}

impl RemoteAddrs {
    fn new(addrs: Vec<SocketAddr>, timeout: Option<Duration>) -> Self {
        Self {
            addrs: DomainAddrs::new(addrs),
            timeout,
        }
    }

    async fn connect(&mut self) -> Result<TcpStream, io::Error> {
        let mut unexpected = None;
        for addr in self.addrs.iter() {
            match connect(addr, self.timeout).await {
                Ok(stream) => {
                    return Ok(stream);
                }
                Err(e) => {
                    unexpected = Some(e);
                }
            }
        }
        match unexpected {
            None => Err(Error::new(ErrorKind::NotConnected, "Invalid domain")),
            Some(e) => Err(e),
        }
    }
}

impl HappyEyeballs {
    pub(crate) fn new(socket_addr: Vec<SocketAddr>, config: EyeBallConfig) -> Self {
        let socket_addr = ResolvedAddrs::new(socket_addr.into_iter());
        // splits SocketAddrs into preferred and other family.
        let (preferred, second) = socket_addr.split_preferred_addrs();
        let preferred_size = preferred.len();
        let second_size = second.len();
        if second.is_empty() {
            HappyEyeballs {
                preferred_addr: RemoteAddrs::new(
                    preferred,
                    config
                        .timeout
                        .and_then(|time| time.checked_div(preferred_size as u32)),
                ),
                delay_addr: None,
            }
        } else {
            let delay = if let Some(delay) = config.delay {
                delay
            } else {
                Duration::from_millis(HAPPY_EYEBALLS_PREFERRED_TIMEOUT_MS)
            };
            HappyEyeballs {
                preferred_addr: RemoteAddrs::new(
                    preferred,
                    config
                        .timeout
                        .and_then(|time| time.checked_div(preferred_size as u32)),
                ),
                // TODO Is it necessary to subtract the delay time
                delay_addr: Some(DelayedAddrs::new(
                    RemoteAddrs::new(
                        second,
                        config
                            .timeout
                            .and_then(|time| time.checked_div(second_size as u32)),
                    ),
                    crate::runtime::sleep(delay),
                )),
            }
        }
    }

    pub(crate) async fn connect(mut self) -> io::Result<TcpStream> {
        match self.delay_addr {
            None => self.preferred_addr.connect().await,
            Some(mut second_addrs) => {
                let preferred_fut = self.preferred_addr.connect();
                let second_fut = second_addrs.addrs.connect();
                let delay_fut = second_addrs.delay;

                #[cfg(feature = "ylong_base")]
                let (stream, stream_fut) = ylong_runtime::select! {
                    preferred = preferred_fut => {
                        (preferred, second_fut)
                    },
                    _ = delay_fut => {
                        let preferred_fut = self.preferred_addr.connect();
                        ylong_runtime::select! {
                            preferred = preferred_fut => {
                                let second_fut = second_addrs.addrs.connect();
                                (preferred, second_fut)
                            },
                            second = second_fut => {
                                let preferred_fut = self.preferred_addr.connect();
                                (second, preferred_fut)
                            },
                        }
                    },
                };

                #[cfg(feature = "tokio_base")]
                let (stream, stream_fut) = tokio::select! {
                    preferred = preferred_fut => {
                        (preferred, second_fut)
                    },
                    _ = delay_fut => {
                        let preferred_fut = self.preferred_addr.connect();
                        tokio::select! {
                            preferred = preferred_fut => {
                                let second_fut = second_addrs.addrs.connect();
                                (preferred, second_fut)
                            },
                            second = second_fut => {
                                let preferred_fut = self.preferred_addr.connect();
                                (second, preferred_fut)
                            },
                        }
                    },
                };

                if stream.is_err() {
                    stream_fut.await
                } else {
                    stream
                }
            }
        }
    }
}

fn connect(
    addr: &SocketAddr,
    timeout: Option<Duration>,
) -> impl Future<Output = io::Result<TcpStream>> {
    let stream_fut = TcpStream::connect(*addr);
    async move {
        match timeout {
            None => stream_fut.await,
            Some(duration) => match crate::runtime::timeout(duration, stream_fut).await {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(io::Error::new(ErrorKind::TimedOut, e)),
            },
        }
    }
}

#[cfg(all(test, feature = "ylong_base"))]
mod ut_dns_happy_eyeballs {
    use std::io;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
    use std::time::{Duration, Instant};

    use crate::async_impl::dns::{EyeBallConfig, HappyEyeballs};

    #[test]
    fn ut_happy_eyeballs_connect() {
        let server_v4_listener = TcpListener::bind("127.0.0.1:0").expect("bind local v4 error");
        let server_v4_addr = server_v4_listener
            .local_addr()
            .expect("get local v4 socket addr error");
        // use dual-stack mode.
        let _server_v6_listener = TcpListener::bind(format!("[::1]:{}", server_v4_addr.port()))
            .expect("bind local v6 error");

        let local_v4_addr = IpAddr::from(Ipv4Addr::new(127, 0, 0, 1));
        let local_v6_addr = IpAddr::from(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        let invalid_v4_addr = IpAddr::from(Ipv4Addr::new(127, 0, 0, 2));
        let invalid_v6_addr = IpAddr::from(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2));
        let internet_v4_addr = IpAddr::from(Ipv4Addr::new(198, 18, 0, 25));

        let default_timeout = Duration::default();
        let invalid_v4_timeout = test_addrs(invalid_v4_addr).1;
        let invalid_v6_timeout = test_addrs(invalid_v6_addr).1;
        let local_v6_timeout = test_addrs(local_v6_addr).1;

        let delay_time = Duration::from_millis(300);

        let test_cases = &[
            (&[local_v4_addr][..], 4, default_timeout),
            (&[local_v6_addr][..], 6, default_timeout),
            (&[local_v4_addr, local_v6_addr][..], 4, default_timeout),
            (&[local_v6_addr, local_v4_addr][..], 6, default_timeout),
            (&[invalid_v4_addr, local_v4_addr][..], 4, invalid_v4_timeout),
            (&[invalid_v6_addr, local_v6_addr][..], 6, invalid_v6_timeout),
            (
                &[invalid_v4_addr, local_v4_addr, local_v6_addr][..],
                4,
                invalid_v4_timeout,
            ),
            (
                &[invalid_v6_addr, local_v6_addr, local_v4_addr][..],
                6,
                invalid_v6_timeout,
            ),
            (
                &[internet_v4_addr, local_v4_addr, local_v6_addr][..],
                6,
                delay_time + local_v6_timeout,
            ),
            (
                &[internet_v4_addr, invalid_v6_addr, local_v6_addr][..],
                6,
                delay_time + invalid_v6_timeout + local_v6_timeout,
            ),
        ];

        for &(hosts, family, timeout) in test_cases {
            let (start, stream) = ylong_runtime::block_on(async move {
                let addrs = hosts
                    .iter()
                    .map(|ip| (*ip, server_v4_addr.port()).into())
                    .collect();
                let config = EyeBallConfig::new(None, Some(delay_time));
                let happy_eyeballs = HappyEyeballs::new(addrs, config);

                let start = Instant::now();
                Ok::<_, io::Error>((start, happy_eyeballs.connect().await?))
            })
            .unwrap();

            let stream_addr_family = if stream.peer_addr().unwrap().is_ipv4() {
                4
            } else {
                6
            };
            let duration = start.elapsed();
            let min_duration = if timeout >= Duration::from_millis(150) {
                timeout - Duration::from_millis(150)
            } else {
                Duration::default()
            };

            let max_duration = timeout + Duration::from_millis(150);

            assert_eq!(stream_addr_family, family);
            assert!(duration >= min_duration);
            assert!(duration <= max_duration);
        }

        fn test_addrs(addr: IpAddr) -> (bool, Duration) {
            let start = Instant::now();
            let reachable = match std::net::TcpStream::connect_timeout(
                &SocketAddr::from((addr, 80)),
                Duration::from_secs(1),
            ) {
                Ok(_) => true,
                Err(err) => err.kind() == io::ErrorKind::TimedOut,
            };
            let duration = start.elapsed();
            (reachable, duration)
        }
    }
}
