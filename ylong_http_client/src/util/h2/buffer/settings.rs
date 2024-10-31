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

//! http2 connection flow control.

use ylong_http::h2::{Frame, H2Error};

use crate::util::h2::buffer::window::RecvWindow;
use crate::util::h2::buffer::SendWindow;

pub(crate) struct FlowControl {
    recv_window: RecvWindow,
    send_window: SendWindow,
}

impl FlowControl {
    pub(crate) fn new(conn_recv_window: u32, conn_send_window: u32) -> Self {
        FlowControl {
            recv_window: RecvWindow::new(conn_recv_window as i32),
            send_window: SendWindow::new(conn_send_window as i32),
        }
    }

    pub(crate) fn check_conn_recv_window_update(&mut self) -> Option<Frame> {
        self.recv_window.check_window_update(0)
    }

    pub(crate) fn setup_recv_window(&mut self, size: u32) {
        let setup = size;
        let actual = self.recv_window.actual_size() as u32;
        if setup > actual {
            let extra = setup - actual;
            self.recv_window.increase_actual(extra);
        } else {
            let extra = actual - setup;
            self.recv_window.reduce_actual(extra);
        }
    }

    pub(crate) fn increase_send_size(&mut self, size: u32) -> Result<(), H2Error> {
        self.send_window.increase_size(size)
    }

    pub(crate) fn send_size_available(&self) -> usize {
        self.send_window.size_available() as usize
    }

    pub(crate) fn recv_notification_size_available(&self) -> u32 {
        self.recv_window.notification_available()
    }

    pub(crate) fn send_data(&mut self, size: u32) {
        self.send_window.send_data(size)
    }

    pub(crate) fn recv_data(&mut self, size: u32) {
        self.recv_window.recv_data(size)
    }
}

#[cfg(test)]
mod ut_flow_control {

    use super::*;

    /// UT test case for `FlowControl::new`.
    ///
    /// # Brief
    /// 1. Create a new `FlowControl` instance with specific receive and send
    ///    window sizes.
    /// 2. Asserts that the initial sizes are correctly set in both windows.
    #[test]
    fn ut_fc_new() {
        let fc = FlowControl::new(100, 200);
        assert_eq!(fc.recv_window.actual_size(), 100);
        assert_eq!(fc.send_window.size_available(), 200);
    }

    /// UT test case for `FlowControl::check_conn_recv_window_update`.
    ///
    /// # Brief
    /// 1. Create a `FlowControl` instance.
    /// 2. Simulates the behavior of `recv_window` and Asserts that
    ///    `check_conn_recv_window_update` returns `None`.
    #[test]
    fn ut_fc_check_conn_recv_window_update() {
        let mut fc = FlowControl::new(100, 200);
        let frame = fc.check_conn_recv_window_update();
        assert!(frame.is_none());
    }

    /// UT test case for `FlowControl::setup_recv_window`
    ///
    /// # Brief
    /// 1. Set receive window size.
    /// 2. Asserts that the receive window size is correctly.
    #[test]
    fn ut_fc_setup_recv_window() {
        let mut fc = FlowControl::new(100, 200);
        fc.setup_recv_window(200);
        assert_eq!(fc.recv_window.actual_size(), 200);
        fc.setup_recv_window(100);
        assert_eq!(fc.recv_window.actual_size(), 100);
    }

    /// UT test case for `FlowControl::increase_send_size`.
    ///
    /// # Brief
    /// 1. Increases the send window size by a specified amount.
    /// 2. Asserts that the size increase is successful and the window is
    ///    updated correctly.
    #[test]
    fn ut_fc_increase_send_size() {
        let mut fc = FlowControl::new(100, 200);
        let res = fc.increase_send_size(50);
        assert!(res.is_ok());
        assert_eq!(fc.send_window.size_available(), 250);
    }

    /// UT test case for `FlowControl::send_size_available`.
    ///
    /// # Brief
    /// 1. Check the available size for sending data in the send window.
    /// 2. Asserts that the available size is as expected.
    #[test]
    fn ut_fc_send_size_available() {
        let fc = FlowControl::new(100, 200);
        assert_eq!(fc.send_size_available(), 200);
    }

    /// UT test case for `FlowControl::recv_notification_size_available`.
    ///
    /// # Brief
    /// 1. Checks the available size for receiving notificaitons in the receive
    ///    window.
    /// 2. Asserts that the available notification size matches the value in
    ///    `recv_window`.
    #[test]
    fn ut_fc_recv_notification_size_available() {
        let fc = FlowControl::new(100, 200);
        let notificaiton_size = fc.recv_notification_size_available();
        assert_eq!(notificaiton_size, fc.recv_window.notification_available());
    }

    /// UT test case for `FlowControl::send_data` and `FlowControl::recv_data`.
    ///
    /// # Brief
    /// 1. Sends data using the send window.
    /// 2. Receives data using the receive window.
    /// 3. Asserts that the available send window and available notification
    ///    size is reduced correctly.
    #[test]
    fn ut_fc_send_and_recv_data() {
        let mut fc = FlowControl::new(100, 200);
        fc.send_data(50);
        fc.recv_data(50);
        assert_eq!(fc.send_window.size_available(), 150);
        assert_eq!(fc.recv_window.notification_available(), 50);
    }
}
