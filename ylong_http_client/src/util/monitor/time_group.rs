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

use std::time::{Duration, Instant};

/// Time statistics of a request in each stage.
#[derive(Default, Clone)]
pub struct TimeGroup {
    // TODO add total request time.
    dns_start: Option<Instant>,
    dns_end: Option<Instant>,
    dns_duration: Option<Duration>,
    tcp_start: Option<Instant>,
    tcp_end: Option<Instant>,
    tcp_duration: Option<Duration>,
    #[cfg(feature = "http3")]
    quic_start: Option<Instant>,
    #[cfg(feature = "http3")]
    quic_end: Option<Instant>,
    #[cfg(feature = "http3")]
    quic_duration: Option<Duration>,
    #[cfg(feature = "__tls")]
    tls_start: Option<Instant>,
    #[cfg(feature = "__tls")]
    tls_end: Option<Instant>,
    #[cfg(feature = "__tls")]
    tls_duration: Option<Duration>,
    conn_start: Option<Instant>,
    conn_end: Option<Instant>,
    conn_duration: Option<Duration>,
    // start send bytes to peer.
    transfer_start: Option<Instant>,
    // received first byte from peer.
    transfer_end: Option<Instant>,
    transfer_duration: Option<Duration>,
}

impl TimeGroup {
    pub(crate) fn set_dns_start(&mut self, start: Instant) {
        self.dns_start = Some(start)
    }

    pub(crate) fn set_dns_end(&mut self, end: Instant) {
        if let Some(start) = self.dns_start {
            self.dns_duration = end.checked_duration_since(start);
        }
        self.dns_end = Some(end)
    }

    pub(crate) fn set_tcp_start(&mut self, start: Instant) {
        self.tcp_start = Some(start)
    }

    pub(crate) fn set_tcp_end(&mut self, end: Instant) {
        if let Some(start) = self.tcp_start {
            self.tcp_duration = end.checked_duration_since(start);
        }
        self.tcp_end = Some(end)
    }

    #[cfg(feature = "http3")]
    pub(crate) fn set_quic_start(&mut self, start: Instant) {
        self.quic_start = Some(start)
    }

    #[cfg(feature = "http3")]
    pub(crate) fn set_quic_end(&mut self, end: Instant) {
        if let Some(start) = self.quic_start {
            self.quic_duration = end.checked_duration_since(start);
        }
        self.quic_end = Some(end)
    }

    #[cfg(feature = "__tls")]
    pub(crate) fn set_tls_start(&mut self, start: Instant) {
        self.tls_start = Some(start)
    }

    #[cfg(feature = "__tls")]
    pub(crate) fn set_tls_end(&mut self, end: Instant) {
        if let Some(start) = self.tls_start {
            self.tls_duration = end.checked_duration_since(start)
        }
        self.tls_end = Some(end)
    }

    pub(crate) fn set_transfer_start(&mut self, start: Instant) {
        self.transfer_start = Some(start)
    }

    pub(crate) fn set_transfer_end(&mut self, end: Instant) {
        if let Some(start) = self.transfer_start {
            self.transfer_duration = end.checked_duration_since(start)
        }
        self.transfer_end = Some(end)
    }

    pub(crate) fn set_connect_start(&mut self, start: Instant) {
        self.conn_start = Some(start)
    }

    pub(crate) fn set_connect_end(&mut self, end: Instant) {
        if let Some(start) = self.conn_start {
            self.conn_duration = end.checked_duration_since(start)
        }
        self.conn_end = Some(end)
    }

    #[cfg(feature = "http3")]
    pub(crate) fn update_quic_start(&mut self, start: Option<Instant>) {
        self.quic_start = start
    }

    #[cfg(feature = "http3")]
    pub(crate) fn update_quic_end(&mut self, end: Option<Instant>) {
        self.quic_end = end
    }

    #[cfg(feature = "http3")]
    pub(crate) fn update_quic_duration(&mut self, duration: Option<Duration>) {
        self.quic_duration = duration
    }

    pub(crate) fn update_tcp_start(&mut self, start: Option<Instant>) {
        self.tcp_start = start
    }

    pub(crate) fn update_tcp_end(&mut self, end: Option<Instant>) {
        self.tcp_end = end
    }

    pub(crate) fn update_tcp_duration(&mut self, duration: Option<Duration>) {
        self.tcp_duration = duration
    }

    #[cfg(feature = "__tls")]
    pub(crate) fn update_tls_start(&mut self, start: Option<Instant>) {
        self.tls_start = start
    }

    #[cfg(feature = "__tls")]
    pub(crate) fn update_tls_end(&mut self, end: Option<Instant>) {
        self.tls_end = end
    }

    #[cfg(feature = "__tls")]
    pub(crate) fn update_tls_duration(&mut self, duration: Option<Duration>) {
        self.tls_duration = duration
    }

    pub(crate) fn update_dns_start(&mut self, start: Option<Instant>) {
        self.dns_start = start
    }

    pub(crate) fn update_dns_end(&mut self, end: Option<Instant>) {
        self.dns_end = end
    }

    pub(crate) fn update_dns_duration(&mut self, duration: Option<Duration>) {
        self.dns_duration = duration
    }

    pub(crate) fn update_connection_start(&mut self, start: Option<Instant>) {
        self.conn_start = start
    }

    pub(crate) fn update_connection_end(&mut self, end: Option<Instant>) {
        self.conn_end = end
    }

    pub(crate) fn update_connection_duration(&mut self, duration: Option<Duration>) {
        self.conn_duration = duration
    }

    pub(crate) fn update_transport_conn_time(&mut self, time_group: &TimeGroup) {
        self.update_dns_start(time_group.dns_start_time());
        self.update_dns_end(time_group.dns_end_time());
        self.update_dns_duration(time_group.dns_duration());

        self.update_tcp_start(time_group.tcp_start_time());
        self.update_tcp_end(time_group.tcp_end_time());
        self.update_tcp_duration(time_group.tcp_duration());

        #[cfg(feature = "http3")]
        self.update_quic_start(time_group.quic_start_time());
        #[cfg(feature = "http3")]
        self.update_quic_end(time_group.quic_end_time());
        #[cfg(feature = "http3")]
        self.update_quic_duration(time_group.quic_duration());

        self.update_tcp_start(time_group.tcp_start_time());
        self.update_tcp_end(time_group.tcp_end_time());
        self.update_tcp_duration(time_group.tcp_duration());

        #[cfg(feature = "__tls")]
        self.update_tls_start(time_group.tls_start_time());
        #[cfg(feature = "__tls")]
        self.update_tls_end(time_group.tls_end_time());
        #[cfg(feature = "__tls")]
        self.update_tls_duration(time_group.tls_duration());

        self.update_connection_start(time_group.connect_start_time());
        self.update_connection_end(time_group.connect_end_time());
        self.update_connection_duration(time_group.connect_duration());
    }

    /// Gets the  point in time when the tcp connection starts to be
    /// established.
    pub fn tcp_start_time(&self) -> Option<Instant> {
        self.tcp_start
    }

    /// Gets the  point in time when the tcp connection was established.
    pub fn tcp_end_time(&self) -> Option<Instant> {
        self.tcp_end
    }

    /// Gets the total time taken to establish a tcp connection.
    pub fn tcp_duration(&self) -> Option<Duration> {
        self.tcp_duration
    }

    /// Gets the  point in time when the quic connection starts to be
    /// established.
    #[cfg(feature = "http3")]
    pub fn quic_start_time(&self) -> Option<Instant> {
        self.quic_start
    }

    /// Gets the  point in time when the quic connection was established.
    #[cfg(feature = "http3")]
    pub fn quic_end_time(&self) -> Option<Instant> {
        self.quic_end
    }

    /// Gets the total time taken to establish a quic connection.
    #[cfg(feature = "http3")]
    pub fn quic_duration(&self) -> Option<Duration> {
        self.quic_duration
    }

    /// Gets the start  point in time of the tls handshake.
    #[cfg(feature = "__tls")]
    pub fn tls_start_time(&self) -> Option<Instant> {
        self.tls_start
    }

    /// Gets the  point in time when the tls connection was established.
    #[cfg(feature = "__tls")]
    pub fn tls_end_time(&self) -> Option<Instant> {
        self.tls_end
    }

    /// Gets the time taken for the tls connection to be established.
    #[cfg(feature = "__tls")]
    pub fn tls_duration(&self) -> Option<Duration> {
        self.tls_duration
    }

    /// Gets the point in time when the dns query started (not currently
    /// recorded).
    pub fn dns_start_time(&self) -> Option<Instant> {
        self.dns_start
    }

    /// Gets the point in time when the dns query ended (not currently
    /// recorded).
    pub fn dns_end_time(&self) -> Option<Instant> {
        self.dns_end
    }

    /// Gets the time spent on dns queries (not currently recorded).
    pub fn dns_duration(&self) -> Option<Duration> {
        self.dns_duration
    }

    /// Gets the start point in time of data transmission.
    pub fn transfer_start_time(&self) -> Option<Instant> {
        self.transfer_start
    }

    /// Gets the point in time the data was received.
    pub fn transfer_end_time(&self) -> Option<Instant> {
        self.transfer_end
    }

    /// Gets the time it takes from the time the data is sent to the time it is
    /// received.
    pub fn transfer_duration(&self) -> Option<Duration> {
        self.transfer_duration
    }

    /// Gets the point in time to start establishing the request connection.
    pub fn connect_start_time(&self) -> Option<Instant> {
        self.conn_start
    }

    /// Gets the point in time when the request connection was established.
    pub fn connect_end_time(&self) -> Option<Instant> {
        self.conn_end
    }

    /// Gets the time it took to establish the requested connection.
    pub fn connect_duration(&self) -> Option<Duration> {
        self.conn_duration
    }
}
