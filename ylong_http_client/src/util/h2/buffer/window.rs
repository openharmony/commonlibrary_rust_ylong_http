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

//! http2 send and recv window definition.

use ylong_http::h2::{ErrorCode, Frame, FrameFlags, H2Error, Payload, StreamId, WindowUpdate};

pub(crate) struct SendWindow {
    // As the sending window, the client retains only its visible window size,
    // and updates only when the SETTINGS frame and WINDOW_UPDATE frame are received from the
    // server.
    size: i32,
}

impl SendWindow {
    pub(crate) fn new(size: i32) -> Self {
        Self { size }
    }

    pub(crate) fn size_available(&self) -> u32 {
        if self.size < 0 {
            0
        } else {
            self.size as u32
        }
    }

    pub(crate) fn reduce_size(&mut self, size: u32) {
        self.size -= size as i32;
    }

    pub(crate) fn increase_size(&mut self, size: u32) -> Result<(), H2Error> {
        let (curr, overflow) = self.size.overflowing_add(size as i32);
        if overflow {
            return Err(H2Error::ConnectionError(ErrorCode::FlowControlError));
        }
        if curr > crate::util::h2::MAX_FLOW_CONTROL_WINDOW as i32 {
            return Err(H2Error::ConnectionError(ErrorCode::FlowControlError));
        }
        self.size = curr;
        Ok(())
    }

    pub(crate) fn send_data(&mut self, size: u32) {
        self.size -= size as i32;
    }
}

#[derive(Default)]
pub(crate) struct RecvWindow {
    // The window size visible to the server.
    // notification decreases the value when a DATA frame is received
    // and increases the value when a WINDOW_UPDATE is sent.
    notification: i32,
    // The window size visible to the client.
    // Since client is a receiving (WINDOW_UPDATE sending) window,
    // the actual remains unchanged except for SETTINGS set by the user updates.
    actual: i32,
}

impl RecvWindow {
    pub(crate) fn new(size: i32) -> Self {
        Self {
            notification: size,
            actual: size,
        }
    }

    pub(crate) fn unreleased_size(&self) -> Option<u32> {
        let unreleased = self.actual - self.notification;
        if unreleased <= 0 {
            return None;
        }
        if unreleased * 2 > self.notification {
            Some(unreleased as u32)
        } else {
            None
        }
    }

    pub(crate) fn actual_size(&self) -> i32 {
        self.actual
    }

    pub(crate) fn notification_available(&self) -> u32 {
        if self.notification < 0 {
            0
        } else {
            self.notification as u32
        }
    }

    pub(crate) fn reduce_actual(&mut self, size: u32) {
        self.actual -= size as i32
    }

    pub(crate) fn increase_actual(&mut self, size: u32) {
        self.actual += size as i32
    }

    pub(crate) fn reduce_notification(&mut self, size: u32) {
        self.notification -= size as i32
    }

    pub(crate) fn increase_notification(&mut self, size: u32) {
        self.notification += size as i32
    }

    pub(crate) fn check_window_update(&mut self, id: StreamId) -> Option<Frame> {
        if let Some(size) = self.unreleased_size() {
            self.increase_notification(size);
            let window_update = WindowUpdate::new(size);
            let frame = Frame::new(id, FrameFlags::new(0), Payload::WindowUpdate(window_update));
            Some(frame)
        } else {
            None
        }
    }

    // The client receiving a DATA frame means that the server has less visible
    // Windows
    pub(crate) fn recv_data(&mut self, size: u32) {
        self.notification -= size as i32;
    }
}

#[cfg(test)]
mod ut_send_window {
    use ylong_http::h2::{ErrorCode, H2Error};

    use super::*;

    /// UT test case for `SendWindow::new`.
    ///
    /// # Brief
    /// 1. Creates a new `SendWindow` instance.
    /// 2. Asserts that the window size is initialized to the provided value.
    #[test]
    fn ut_sw_new() {
        let sw = SendWindow::new(100);
        assert_eq!(sw.size, 100);
    }

    /// UT test case for `SendWindow::size_available`.
    ///
    /// # Brief
    /// 1. Creates a new `SendWindow` instance.
    /// 2. Checks that the available size is returns correctly.
    #[test]
    fn ut_sw_size_available() {
        let sw = SendWindow::new(100);
        assert_eq!(sw.size_available(), 100);
        let sw = SendWindow::new(-1);
        assert_eq!(sw.size_available(), 0);
    }

    /// UT test case for `SendWindow::reduce_size`.
    ///
    /// # Brief
    /// 1. Reduces the send window size by a specified value.
    /// 2. Asserts that the size is correctly reduce.
    #[test]
    fn ut_sw_reduce_size() {
        let mut sw = SendWindow::new(100);
        sw.reduce_size(50);
        assert_eq!(sw.size, 50);
    }

    /// UT test case for `SendWindow::increase_size`.
    ///
    /// # Brief
    /// 1. Increases the send window size.
    /// 2. Asserts that the size is increased correctly.
    /// 3. Attempts to increase the window size beyond the maximum allowable
    ///    value or beyond the maximum flow control window size.
    /// 4. Asserts that the operation fails.
    #[test]
    fn ut_sw_window_increase_size() {
        let mut sw = SendWindow::new(100);
        assert!(sw.increase_size(50).is_ok());
        assert_eq!(sw.size, 150);

        let mut sw = SendWindow::new(i32::MAX);
        let res = sw.increase_size(1);
        assert!(res.is_err());
        if let Err(H2Error::ConnectionError(code)) = res {
            assert_eq!(code, ErrorCode::FlowControlError);
        }

        let mut sw = SendWindow::new(1);
        let res = sw.increase_size(crate::util::h2::MAX_FLOW_CONTROL_WINDOW);
        assert!(res.is_err());
        assert_eq!(
            res,
            Err(H2Error::ConnectionError(ErrorCode::FlowControlError))
        );
    }

    /// UT test case for `SendWindow::send_data`.
    ///
    /// # Brief
    /// 1. Sends data by reducing the send window size.
    /// 2. Asserts that the send window size is correctly redueced after the
    ///    data is sent.
    #[test]
    fn ut_sw_send_data() {
        let mut sw = SendWindow::new(100);
        sw.send_data(50);
        assert_eq!(sw.size, 50);
    }
}

#[cfg(test)]
mod ut_recv_window {
    use super::*;

    /// UT test case for `RecvWindow::new`.
    ///
    /// # Brief
    /// 1. Creates a new `RecvWindow` instance.
    /// 2. Asserts that both the `notification` and `actual` sizes are
    ///    initialized to the provided value.
    #[test]
    fn ut_rw_new() {
        let rw = RecvWindow::new(100);
        assert_eq!(rw.notification, 100);
        assert_eq!(rw.actual, 100);
    }

    /// UT test case for `RecvWindow::unreleased_size`.
    ///
    /// # Brief
    /// 1. Creates a `RecvWindow` instance.
    /// 2. Asserts that no unreleased size is reported when the notification is
    ///    greater than or equal to the actual size.
    /// 3. Simulate a scenario where the notification size is smaller than the
    ///    actual size and asserts that unreleased size is repoeted.
    /// 4. Simulate a scenario where the notification size is slightly less than
    ///    the actual size but still no unreleased size is reported.
    #[test]
    fn ut_rw_unreleased_size() {
        let mut rw = RecvWindow::new(100);
        assert_eq!(rw.unreleased_size(), None);
        rw.notification = 50;
        assert_eq!(rw.unreleased_size(), Some(50));
        rw.notification = 80;
        assert_eq!(rw.unreleased_size(), None);
    }

    /// UT test case for `RecvWindow::actual_size`.
    ///
    /// # Brief
    /// 1. Retrieves the actual window size.
    /// 2. Asserts that the size is returned correctly.
    #[test]
    fn ut_rw_actual_size() {
        let rw = RecvWindow::new(100);
        assert_eq!(rw.actual_size(), 100);
    }

    /// UT test case for `RecvWindow::notification_available`.
    ///
    /// # Brief
    /// 1. Asserts that the available notification size is correctly reported
    ///    for a positive value.
    /// 2. Simulates a scenario where the notification size is negative and
    ///    asserts that the available notification size reported as zero.
    #[test]
    fn ut_rw_notification_available() {
        let rw = RecvWindow::new(100);
        assert_eq!(rw.notification_available(), 100);
        let rw = RecvWindow::new(-1);
        assert_eq!(rw.notification_available(), 0);
    }

    /// UT test case for `RecvWindow::{reduce_actual,increase_actual}`
    ///
    /// # Brief
    /// 1. Reduces and increase the actual window size.
    /// 2. Asserts that the size is correctly reduced and increased.
    #[test]
    fn ut_rw_reduce_and_increase_actual() {
        let mut rw = RecvWindow::new(100);
        rw.reduce_actual(50);
        assert_eq!(rw.actual, 50);
        rw.increase_actual(50);
        assert_eq!(rw.actual, 100);
    }

    /// UT test case for
    /// `RecvWindow::{reduce_notification,increase_notificaiton}`
    ///
    /// # Brief
    /// 1. Reduces and increase the notification window size.
    /// 2. Asserts that the size is correctly reduced and increased.
    #[test]
    fn ut_rw_reduce_and_increase_notification() {
        let mut rw = RecvWindow::new(100);
        rw.reduce_notification(50);
        assert_eq!(rw.notification, 50);
        rw.increase_notification(50);
        assert_eq!(rw.notification, 100);
    }

    /// UT test case for `RecvWindow::check_window_update`.
    ///
    /// # Brief
    /// 1. Checks for a window update when there is no unreleased size.
    /// 2. Asserts that no `Frame` is generated.
    /// 3. Checks for a window update when there is unreleased size available.
    /// 4. Asserts that a `Frame` is generated for the window update.
    #[test]
    fn ut_rw_check_window_update() {
        let mut rw = RecvWindow::new(100);
        let frame = rw.check_window_update(1);
        assert!(frame.is_none());
        rw.notification = 50;
        let frame = rw.check_window_update(1);
        assert!(frame.is_some());
    }

    /// UT test case for `RecvWindow::recv_data`.
    ///
    /// # Brief
    /// 1. Simulates receiving data, whice reduces the notification size.
    /// 2. Asserts that the notification size is correctly reduced after
    ///    receiving data.
    #[test]
    fn ut_rw_send_data() {
        let mut rw = RecvWindow::new(100);
        rw.recv_data(50);
        assert_eq!(rw.notification, 50);
    }
}
