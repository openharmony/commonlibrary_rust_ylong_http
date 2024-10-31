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
