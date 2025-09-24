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

#[cfg(target_os = "linux")]
use libc::{gid_t, uid_t};

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

    #[cfg(all(target_os = "linux", feature = "ylong_base", feature = "__tls"))]
    async fn connect_with_owner(&mut self, uid: uid_t, gid: gid_t) -> Result<TcpStream, io::Error> {
        let mut unexpected = None;
        for addr in self.addrs.iter() {
            match connect_with_owner(addr, self.timeout, uid, gid).await {
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

    #[cfg(all(target_os = "linux", feature = "ylong_base", feature = "__tls"))]
    pub(crate) async fn connect_with_owner(
        mut self,
        uid: uid_t,
        gid: gid_t,
    ) -> io::Result<TcpStream> {
        match self.delay_addr {
            None => self.preferred_addr.connect_with_owner(uid, gid).await,
            Some(mut second_addrs) => {
                let preferred_fut = self.preferred_addr.connect_with_owner(uid, gid);
                let second_fut = second_addrs.addrs.connect_with_owner(uid, gid);
                let delay_fut = second_addrs.delay;

                let (stream, stream_fut) = ylong_runtime::select! {
                    preferred = preferred_fut => {
                        (preferred, second_fut)
                    },
                    _ = delay_fut => {
                        let preferred_fut = self.preferred_addr.connect_with_owner(uid, gid);
                        ylong_runtime::select! {
                            preferred = preferred_fut => {
                                let second_fut = second_addrs.addrs.connect_with_owner(uid, gid);
                                (preferred, second_fut)
                            },
                            second = second_fut => {
                                let preferred_fut = self.preferred_addr.connect_with_owner(uid, gid);
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

#[cfg(all(target_os = "linux", feature = "ylong_base", feature = "__tls"))]
fn connect_with_owner(
    addr: &SocketAddr,
    timeout: Option<Duration>,
    uid: uid_t,
    gid: gid_t,
) -> impl Future<Output = io::Result<TcpStream>> {
    let stream_fut = TcpStream::connect_with_owner(*addr, uid, gid);
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
