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

use std::ops::{Deref, DerefMut};

use crate::h3::Frame;

/// HTTP3 control stream type code.
pub const CONTROL_STREAM_TYPE: u8 = 0x0;
/// HTTP3 push stream type code.
pub const PUSH_STREAM_TYPE: u8 = 0x1;
/// qpack encoder stream type code.
pub const QPACK_ENCODER_STREAM_TYPE: u8 = 0x2;
/// qpack decoder stream type code.
pub const QPACK_DECODER_STREAM_TYPE: u8 = 0x3;

/// Http3 decoded frames.
pub struct Frames {
    list: Vec<FrameKind>,
}

/// An iterator of `Frames`.
pub struct FramesIter<'a> {
    iter: core::slice::Iter<'a, FrameKind>,
}

/// A consuming iterator of `Frames`.
pub struct FramesIntoIter {
    into_iter: std::vec::IntoIter<FrameKind>,
}

/// Qpack decoder the state after decoding a frame.
pub enum FrameKind {
    /// PUSH_PROMISE or HEADERS frame parsing completed.
    Complete(Box<Frame>),
    /// Partial decoded of PUSH_PROMISE or HEADERS frame.
    Partial,
    /// Headers part is blocked at Qpack decode.
    Blocked,
}

impl Frames {
    /// Gets an iterator for fFrames.
    pub fn iter(&self) -> FramesIter {
        FramesIter {
            iter: self.list.iter(),
        }
    }

    /// Returns the size of `Frames`.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Checks if the `Frames` is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Frames {
    pub(crate) fn new() -> Self {
        Frames { list: vec![] }
    }
    pub(crate) fn push(&mut self, frame: FrameKind) {
        self.list.push(frame)
    }
}

impl<'a> Deref for FramesIter<'a> {
    type Target = core::slice::Iter<'a, FrameKind>;

    fn deref(&self) -> &Self::Target {
        &self.iter
    }
}

impl<'a> DerefMut for FramesIter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.iter
    }
}

impl<'a> Iterator for FramesIter<'a> {
    type Item = &'a FrameKind;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl Iterator for FramesIntoIter {
    type Item = FrameKind;

    fn next(&mut self) -> Option<Self::Item> {
        self.into_iter.next()
    }
}

impl core::iter::IntoIterator for Frames {
    type Item = FrameKind;
    type IntoIter = FramesIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        FramesIntoIter {
            into_iter: self.list.into_iter(),
        }
    }
}

/// The Http3 decoder deserializes the data.
pub enum StreamMessage {
    /// Request stream message.
    Request(Frames),
    /// Control stream message.
    Control(Frames),
    /// Push stream message.
    Push(u64, Frames),
    /// Qpack encoder stream message.
    QpackEncoder(Vec<u64>),
    /// Qpack decoder stream message.
    QpackDecoder(Vec<u8>),
    /// Unknown stream message.
    Unknown,
    /// Bytes too short to decode a stream.
    WaitingMore,
}
