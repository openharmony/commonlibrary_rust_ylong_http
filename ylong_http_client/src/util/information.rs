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

use std::net::SocketAddr;

#[cfg(feature = "http3")]
use crate::async_impl::QuicConn;
use crate::{ConnProtocol, TimeGroup};

/// `ConnDetail` trait, which is used to obtain information about the current
/// connection.
pub trait ConnInfo {
    /// Whether the current connection is a proxy.
    fn is_proxy(&self) -> bool;

    /// Gets connection information data.
    fn conn_data(&self) -> ConnData;

    /// Gets quic information
    #[cfg(feature = "http3")]
    fn quic_conn(&mut self) -> Option<QuicConn>;
}

/// Tcp connection information.
#[derive(Clone)]
pub struct ConnDetail {
    /// Transport layer protocol type.
    pub(crate) protocol: ConnProtocol,
    /// local socket address.
    pub(crate) local: SocketAddr,
    /// peer socket address.
    pub(crate) peer: SocketAddr,
    /// peer domain information.
    pub(crate) addr: String,
}

impl ConnDetail {
    /// Gets the transport layer protocol for the connection.
    pub fn protocol(&self) -> &ConnProtocol {
        &self.protocol
    }

    /// Gets the local socket address of the connection.
    pub fn local(&self) -> SocketAddr {
        self.local
    }

    /// Gets the peer socket address of the connection.
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Gets the peer domain address of the connection.
    pub fn addr(&self) -> &str {
        &self.addr
    }
}

/// Negotiated http version information.
#[derive(Default, Clone)]
pub struct NegotiateInfo {
    alpn: Option<Vec<u8>>,
}

impl NegotiateInfo {
    /// Constructs NegotiateInfo with apln extensions.
    #[cfg(feature = "__tls")]
    pub fn from_alpn(alpn: Option<Vec<u8>>) -> Self {
        Self { alpn }
    }

    /// tls alpn Indicates extended information.
    pub fn alpn(&self) -> Option<&[u8]> {
        self.alpn.as_deref()
    }
}

/// Transport layer connection establishment information data.
#[derive(Clone)]
pub struct ConnData {
    detail: ConnDetail,
    #[cfg(feature = "http2")]
    negotiate: NegotiateInfo,
    proxy: bool,
    time_group: TimeGroup,
}

impl ConnData {
    /// Construct a `ConnDataBuilder`.
    pub fn builder() -> ConnDataBuilder {
        ConnDataBuilder::default()
    }

    pub(crate) fn detail(self) -> ConnDetail {
        self.detail
    }

    #[cfg(feature = "http2")]
    pub(crate) fn negotiate(&self) -> &NegotiateInfo {
        &self.negotiate
    }

    pub(crate) fn is_proxy(&self) -> bool {
        self.proxy
    }

    pub(crate) fn time_group_mut(&mut self) -> &mut TimeGroup {
        &mut self.time_group
    }
}

/// ConnData's builder, which builds ConnData through cascading calls.
#[derive(Default)]
pub struct ConnDataBuilder {
    #[cfg(feature = "http2")]
    negotiate: NegotiateInfo,
    proxy: bool,
    time_group: TimeGroup,
}

impl ConnDataBuilder {
    /// Sets the http negotiation result.
    #[cfg(all(feature = "__tls", feature = "http2"))]
    pub fn negotiate(mut self, negotiate: NegotiateInfo) -> Self {
        self.negotiate = negotiate;
        self
    }

    /// Sets whether the peer is a proxy.
    pub fn proxy(mut self, proxy: bool) -> Self {
        self.proxy = proxy;
        self
    }

    /// Set the time required for each phase of connection establishment.
    pub fn time_group(mut self, time_group: TimeGroup) -> Self {
        self.time_group = time_group;
        self
    }

    /// Construct ConnData by setting the individual endpoint information.
    pub fn build(self, detail: ConnDetail) -> ConnData {
        ConnData {
            detail,
            #[cfg(feature = "http2")]
            negotiate: self.negotiate,
            proxy: self.proxy,
            time_group: self.time_group,
        }
    }
}
