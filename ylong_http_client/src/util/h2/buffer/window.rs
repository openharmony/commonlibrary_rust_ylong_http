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

use ylong_http::h2::{ErrorCode, Frame, FrameFlags, H2Error, Payload, WindowUpdate};

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

    pub(crate) fn check_window_update(&mut self, id: u32) -> Option<Frame> {
        if let Some(size) = self.unreleased_size() {
            self.increase_notification(size);
            let window_update = WindowUpdate::new(size);
            let frame = Frame::new(
                id as usize,
                FrameFlags::new(0),
                Payload::WindowUpdate(window_update),
            );
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
