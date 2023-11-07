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

#![rustfmt::skip]

use crate::h3::qpack::error::{ErrorCode, H3errorQpack};
use std::cmp::Ordering;

pub(crate) struct Integer {
    pub(crate) int: IntegerEncoder,
}

impl Integer {
    pub(crate) fn index(pre: u8, index: usize, mask: u8) -> Self {
        Self {
            int: IntegerEncoder::new(pre, index, mask),
        }
    }
    pub(crate) fn length(length: usize, is_huffman: bool) -> Self {
        Self {
            int: IntegerEncoder::new(u8::from(is_huffman), length, 0x7f),
        }
    }
    pub(crate) fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;
        while !self.int.is_finish() {
            let dst = &mut dst[cur..];
            if dst.is_empty() {
                return Err(self);
            }
            dst[0] = self.int.next_byte().unwrap();
            cur += 1;
        }
        Ok(cur)
    }
}
pub(crate) struct IntegerDecoder {
    index: usize,
    shift: u32,
}

impl IntegerDecoder {
    /// Calculates an integer based on the incoming first byte and mask.
    /// If no subsequent bytes exist, return the result directly, otherwise
    /// return the decoder itself.
    pub(crate) fn first_byte(byte: u8, mask: u8) -> Result<usize, Self> {
        let index = byte & mask;
        match index.cmp(&mask) {
            Ordering::Less => Ok(index as usize),
            _ => Err(Self {
                index: index as usize,
                shift: 1,
            }),
        }
    }

    /// Continues computing the integer based on the next byte of the input.
    /// Returns `Ok(Some(index))` if the result is obtained, otherwise returns
    /// `Ok(None)`, and returns Err in case of overflow.
    pub(crate) fn next_byte(&mut self, byte: u8) -> Result<Option<usize>, H3errorQpack> {
        self.index = 1usize
            .checked_shl(self.shift - 1)
            .and_then(|res| res.checked_mul((byte & 0x7f) as usize))
            .and_then(|res| res.checked_add(self.index))
            .ok_or(H3errorQpack::ConnectionError(
                ErrorCode::DecompressionFailed,
            ))?; //todo: modify the error code
        self.shift += 7;
        match (byte & 0x80) == 0x00 {
            true => Ok(Some(self.index)),
            false => Ok(None),
        }
    }
}

pub(crate) struct IntegerEncoder {
    pre: u8,
    i: usize,
    mask: u8,
    state: IntegerEncodeState,
}

/// Enumeration of states that the `IntegerEncoder` needs to use.
enum IntegerEncodeState {
    First,
    Other,
    Finish,
}

impl IntegerEncoder {
    /// Creates a new `IntegerEncoder`.
    pub(crate) fn new(pre: u8, i: usize, mask: u8) -> Self {
        Self {
            pre,
            i,
            mask,
            state: IntegerEncodeState::First,
        }
    }

    /// return the value of the integer
    pub(crate) fn get_index(&self) -> usize {
        self.i
    }
    pub(crate) fn get_pre(&self) -> u8 {
        self.pre
    }

    /// Gets the next byte of the integer. If no remaining bytes are calculated,
    /// return `None`.
    pub(crate) fn next_byte(&mut self) -> Option<u8> {
        match self.state {
            IntegerEncodeState::First => {
                if self.i < self.mask as usize {
                    self.state = IntegerEncodeState::Finish;
                    return Some(self.pre | (self.i as u8));
                }
                self.i -= self.mask as usize;
                self.state = IntegerEncodeState::Other;
                Some(self.pre | self.mask)
            }
            IntegerEncodeState::Other => Some(if self.i >= 128 {
                let res = (self.i & 0x7f) as u8;
                self.i >>= 7;
                res | 0x80
            } else {
                self.state = IntegerEncodeState::Finish;
                (self.i & 0x7f) as u8
            }),
            IntegerEncodeState::Finish => None,
        }
    }

    /// Checks if calculation is over.
    pub(crate) fn is_finish(&self) -> bool {
        matches!(self.state, IntegerEncodeState::Finish)
    }
}
