// Copyright (c) 2024 Huawei Device Co., Ltd.
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

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::runtime::{sleep, Sleep};
use crate::HttpClientError;

pub(crate) const SPEED_CHECK_PERIOD: Duration = Duration::from_millis(1000);

#[derive(Default, Clone)]
pub(crate) struct SpeedController {
    pub(crate) send_rate_limit: RateLimit,
    pub(crate) recv_rate_limit: RateLimit,
}

impl SpeedController {
    pub(crate) fn none() -> Self {
        SpeedController::default()
    }

    pub(crate) fn set_speed_limit(&mut self, config: SpeedConfig) {
        if let Some(speed) = config.max_recv_speed() {
            self.recv_rate_limit
                .set_max_speed(speed, SPEED_CHECK_PERIOD);
        }

        if let Some(speed) = config.min_recv_speed() {
            if let Some(interval) = config.min_speed_interval() {
                self.recv_rate_limit.set_min_speed(
                    speed,
                    SPEED_CHECK_PERIOD,
                    Duration::from_secs(interval),
                );
            }
        }

        if let Some(speed) = config.max_send_speed() {
            self.send_rate_limit
                .set_max_speed(speed, SPEED_CHECK_PERIOD);
        }

        if let Some(speed) = config.min_send_speed() {
            if let Some(interval) = config.min_speed_interval() {
                self.send_rate_limit.set_min_speed(
                    speed,
                    SPEED_CHECK_PERIOD,
                    Duration::from_secs(interval),
                );
            }
        }
    }

    pub(crate) fn need_limit_max_send_speed(&self) -> bool {
        self.send_rate_limit.need_limit_max_speed()
    }

    pub(crate) async fn max_send_speed_limit(&mut self, size: usize) {
        self.send_rate_limit.max_speed_limit(size).await
    }

    pub(crate) fn delay_max_recv_speed_limit(&mut self, size: usize) {
        self.recv_rate_limit.delay_max_speed_limit(size)
    }

    #[cfg(any(feature = "http2", feature = "http3"))]
    pub(crate) fn delay_max_send_speed_limit(&mut self, size: usize) {
        self.send_rate_limit.delay_max_speed_limit(size)
    }

    pub(crate) fn min_send_speed_limit(&mut self, size: usize) -> Result<(), HttpClientError> {
        self.send_rate_limit.min_speed_limit(size)
    }

    pub(crate) fn reset_send_pending_timeout(&mut self) {
        self.send_rate_limit.reset_pending_timeout()
    }

    pub(crate) fn min_recv_speed_limit(&mut self, size: usize) -> Result<(), HttpClientError> {
        self.recv_rate_limit.min_speed_limit(size)
    }

    pub(crate) fn reset_recv_pending_timeout(&mut self) {
        self.recv_rate_limit.reset_pending_timeout()
    }

    pub(crate) fn poll_max_recv_delay_time(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.recv_rate_limit.poll_limited_delay(cx)
    }

    pub(crate) fn poll_recv_pending_timeout(&mut self, cx: &mut Context<'_>) -> bool {
        self.recv_rate_limit.poll_pending_timeout(cx)
    }

    pub(crate) fn poll_send_pending_timeout(&mut self, cx: &mut Context<'_>) -> bool {
        self.send_rate_limit.poll_pending_timeout(cx)
    }

    #[cfg(any(feature = "http2", feature = "http3"))]
    pub(crate) fn poll_max_send_delay_time(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.send_rate_limit.poll_limited_delay(cx)
    }

    pub(crate) fn init_max_send_if_not_start(&mut self) {
        self.send_rate_limit.init_max_limit_if_not_start();
    }

    pub(crate) fn init_min_send_if_not_start(&mut self) {
        self.send_rate_limit.init_min_limit_if_not_start();
    }

    pub(crate) fn init_max_recv_if_not_start(&mut self) {
        self.recv_rate_limit.init_max_limit_if_not_start();
    }

    pub(crate) fn init_min_recv_if_not_start(&mut self) {
        self.recv_rate_limit.init_min_limit_if_not_start();
    }
}

#[derive(Default, Clone)]
pub(crate) struct RateLimit {
    min_speed: Option<SpeedLimit>,
    max_speed: Option<SpeedLimit>,
}

impl RateLimit {
    pub(crate) fn set_min_speed(&mut self, rate: u64, period: Duration, interval: Duration) {
        let limit = SpeedLimit::new(rate, period, interval);
        self.min_speed = Some(limit)
    }

    pub(crate) fn set_max_speed(&mut self, rate: u64, period: Duration) {
        let limit = SpeedLimit::new(rate, period, Duration::default());
        self.max_speed = Some(limit)
    }

    pub(crate) fn need_limit_max_speed(&self) -> bool {
        self.max_speed.is_some()
    }

    pub(crate) fn init_max_limit_if_not_start(&mut self) {
        if let Some(ref mut speed) = self.max_speed {
            speed.init_if_not_start()
        }
    }

    pub(crate) fn init_min_limit_if_not_start(&mut self) {
        if let Some(ref mut speed) = self.min_speed {
            speed.init_if_not_start()
        }
    }

    pub(crate) async fn max_speed_limit(&mut self, read: usize) {
        if let Some(ref mut speed) = self.max_speed {
            speed.limit_max_speed(read).await
        }
    }

    pub(crate) fn delay_max_speed_limit(&mut self, read: usize) {
        if let Some(ref mut speed) = self.max_speed {
            speed.delay_max_speed_limit(read)
        }
    }

    pub(crate) fn min_speed_limit(&mut self, read: usize) -> Result<(), HttpClientError> {
        if let Some(ref mut speed) = self.min_speed {
            speed.limit_min_speed(read)
        } else {
            Ok(())
        }
    }

    pub(crate) fn reset_pending_timeout(&mut self) {
        if let Some(ref mut speed) = self.min_speed {
            speed.reset_pending_timeout()
        }
    }

    pub(crate) fn poll_pending_timeout(&mut self, cx: &mut Context<'_>) -> bool {
        self.min_speed
            .as_mut()
            .is_some_and(|speed| speed.poll_pending_timeout(cx))
    }

    pub(crate) fn poll_limited_delay(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(ref mut speed) = self.max_speed {
            return speed.poll_max_limited_delay(cx);
        }
        Poll::Ready(())
    }
}

#[derive(Default)]
pub(crate) struct SpeedLimit {
    rate: u64,
    // Speed limiting period, millisecond.
    period: Duration,
    min_speed_interval: Duration,
    // min_speed_interval start time.
    min_speed_start: Option<Instant>,
    // Data received within a period, byte.
    period_data: u64,
    // The elapsed time in the period.
    elapsed_time: Duration,
    // The maximum data allowed within a period, byte.
    max_speed_allowed_bytes: u64,
    // The start time of each io read or write.
    start: Option<Instant>,
    // The time delay required to trigger the maximum speed limit.
    delay: Option<Pin<Box<Sleep>>>,
    // min_speed_interval Pending Timeout time.
    timeout: Option<Pin<Box<Sleep>>>,
}

impl SpeedLimit {
    /// Creates a new `SpeedLimit`.
    /// `rate` is the download size allowed within a period, expressed in
    /// bytes/second.
    pub(crate) fn new(rate: u64, period: Duration, interval: Duration) -> SpeedLimit {
        SpeedLimit {
            rate,
            period,
            min_speed_interval: interval,
            min_speed_start: None,
            period_data: 0,
            elapsed_time: Duration::default(),
            max_speed_allowed_bytes: rate * period.as_secs(),
            start: None,
            delay: None,
            timeout: Some(Box::pin(sleep(interval))),
        }
    }

    pub(crate) fn init_if_not_start(&mut self) {
        self.start.get_or_insert(Instant::now());
    }

    pub(crate) fn poll_pending_timeout(&mut self, cx: &mut Context<'_>) -> bool {
        self.timeout
            .as_mut()
            .is_some_and(|timeout| Pin::new(timeout).poll(cx).is_ready())
    }

    pub(crate) fn poll_max_limited_delay(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(delay) = self.delay.as_mut() {
            return match Pin::new(delay).poll(cx) {
                Poll::Ready(()) => {
                    self.delay = None;
                    self.next_period();
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            };
        }
        Poll::Ready(())
    }

    pub(crate) fn delay_max_speed_limit(&mut self, data_size: usize) {
        if let Some(start_time) = self.start.take() {
            self.elapsed_time += start_time.elapsed();
            self.period_data += data_size as u64;
            if self.elapsed_time < self.period {
                if self.period_data >= self.max_speed_allowed_bytes {
                    // The minimum milliseconds to download this data within the speed limit.
                    let limited_time = Duration::from_millis(self.period_data * 1000 / self.rate);
                    // We will not poll here immediately because the data has not yet been returned
                    // to user.
                    self.delay = Some(Box::pin(sleep(limited_time - self.elapsed_time)));
                }
            } else {
                // The minimum milliseconds to download this data within the speed limit.
                let limited_time = Duration::from_millis(self.period_data * 1000 / self.rate);
                if self.elapsed_time < limited_time {
                    // We will not poll here immediately because the data has not yet been returned
                    // to user.
                    self.delay = Some(Box::pin(sleep(limited_time - self.elapsed_time)));
                } else {
                    // We don't count the part that goes beyond the period, and we go straight to
                    // the next period.
                    self.next_period()
                }
            }
        }
    }

    pub(crate) async fn limit_max_speed(&mut self, data_size: usize) {
        if let Some(start_time) = self.start.take() {
            // let elapsed_total = start_time.elapsed();
            self.elapsed_time += start_time.elapsed();
            self.period_data += data_size as u64;
            if self.elapsed_time < self.period {
                if self.period_data >= self.max_speed_allowed_bytes {
                    // The minimum milliseconds to download this data within the speed limit.
                    let limited_time = Duration::from_millis(self.period_data * 1000 / self.rate);
                    sleep(limited_time - self.elapsed_time).await;
                    self.next_period();
                }
            } else {
                // The minimum milliseconds to download this data within the speed limit.
                let limited_time = Duration::from_millis(self.period_data * 1000 / self.rate);
                if self.elapsed_time < limited_time {
                    sleep(limited_time - self.elapsed_time).await;
                }
                // We don't count the part that goes beyond the period, and we go straight to
                // the next period.
                self.next_period()
            }
        }
    }

    pub(crate) fn limit_min_speed(&mut self, data_size: usize) -> Result<(), HttpClientError> {
        if let Some(start_time) = self.start.take() {
            self.min_speed_start.get_or_insert(start_time);
            self.elapsed_time += start_time.elapsed();
            if self.elapsed_time >= self.period {
                self.check_min_speed(data_size)?;
            } else {
                self.period_data += data_size as u64;
            }
        }
        Ok(())
    }

    pub(crate) fn reset_pending_timeout(&mut self) {
        self.timeout = Some(Box::pin(sleep(self.min_speed_interval)));
    }

    fn check_min_speed(&mut self, data_size: usize) -> Result<(), HttpClientError> {
        self.period_data += data_size as u64;
        // The time it takes to process period_data at the minimum speed limit.
        let limited_time = Duration::from_millis(self.period_data * 1000 / self.rate);
        if self.elapsed_time > limited_time {
            // self.min_speed_start must be Some because it was assigned before this
            // function was called.
            if let Some(ref check_start) = self.min_speed_start {
                let check_elapsed = check_start.elapsed();
                // If the time at min_speed_limit exceeds min_speed_interval, an error is
                // raised.
                if check_elapsed > self.min_speed_interval {
                    return err_from_msg!(BodyTransfer, "Below low speed limit");
                }
            }
        } else {
            // If the speed exceeds min_speed_limit, min_speed_interval is reset
            // immediately.
            self.next_interval();
        }
        self.next_period();
        Ok(())
    }

    fn next_period(&mut self) {
        self.period_data = 0;
        self.start = None;
        self.elapsed_time = Duration::default();
    }

    fn next_interval(&mut self) {
        self.min_speed_start = None
    }
}

impl Clone for SpeedLimit {
    fn clone(&self) -> Self {
        Self {
            rate: self.rate,
            period: self.period,
            min_speed_interval: self.min_speed_interval,
            min_speed_start: None,
            period_data: self.period_data,
            elapsed_time: self.elapsed_time,
            max_speed_allowed_bytes: self.max_speed_allowed_bytes,
            start: None,
            delay: None,
            timeout: None,
        }
    }
}

#[derive(Default, Copy, Clone)]
pub(crate) struct SpeedConfig {
    max_recv: Option<u64>,
    min_recv: Option<u64>,
    max_send: Option<u64>,
    min_send: Option<u64>,
    min_speed_interval: Option<u64>,
}

impl SpeedConfig {
    pub(crate) fn none() -> SpeedConfig {
        Self::default()
    }

    pub(crate) fn set_max_rate(&mut self, rate: u64) {
        self.max_recv = Some(rate);
        self.max_send = Some(rate)
    }

    pub(crate) fn set_min_rate(&mut self, rate: u64) {
        self.min_send = Some(rate);
        self.min_recv = Some(rate)
    }

    pub(crate) fn set_min_speed_interval(&mut self, seconds: u64) {
        self.min_speed_interval = Some(seconds)
    }

    pub(crate) fn max_recv_speed(&self) -> Option<u64> {
        self.max_recv
    }

    pub(crate) fn max_send_speed(&self) -> Option<u64> {
        self.max_send
    }

    pub(crate) fn min_recv_speed(&self) -> Option<u64> {
        self.min_recv
    }

    pub(crate) fn min_send_speed(&self) -> Option<u64> {
        self.min_send
    }

    pub(crate) fn min_speed_interval(&self) -> Option<u64> {
        self.min_speed_interval
    }
}
