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

//! [Integer Representation] implementation of [HPACK].
//!
//! [Integer Representation]: https://httpwg.org/specs/rfc7541.html#integer.representation
//! [HPACK]: https://httpwg.org/specs/rfc7541.html
//!
//! # Introduction
//! Integers are used to represent name indexes, header field indexes, or
//! string lengths. An integer representation can start anywhere within an
//! octet. To allow for optimized processing, an integer representation always
//! finishes at the end of an octet.

use core::cmp::Ordering;

use crate::h2::error::ErrorCode;
use crate::h2::H2Error;

/// `IntegerDecoder` implementation according to `Pseudocode to decode an
/// integer I` in `RFC7541 section-5.1`.
///
/// # Pseudocode
/// ```text
/// decode I from the next N bits
/// if I < 2^N - 1, return I
/// else
///     M = 0
///     repeat
///         B = next octet
///         I = I + (B & 127) * 2^M
///         M = M + 7
///     while B & 128 == 128
///     return I
/// ```
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
    pub(crate) fn next_byte(&mut self, byte: u8) -> Result<Option<usize>, H2Error> {
        self.index = 1usize
            .checked_shl(self.shift - 1)
            .and_then(|res| res.checked_mul((byte & 0x7f) as usize))
            .and_then(|res| res.checked_add(self.index))
            .ok_or(H2Error::ConnectionError(ErrorCode::CompressionError))?;
        self.shift += 7;
        match (byte & 0x80) == 0x00 {
            true => Ok(Some(self.index)),
            false => Ok(None),
        }
    }
}

/// `IntegerEncoder` implementation according to `Pseudocode to represent an
/// integer I` in `RFC7541 section-5.1`.
///
/// # Pseudocode
/// ```text
/// if I < 2^N - 1, encode I on N bits
/// else
///     encode (2^N - 1) on N bits
///     I = I - (2^N - 1)
///     while I >= 128
///          encode (I % 128 + 128) on 8 bits
///          I = I / 128
///     encode I on 8 bits
/// ```
pub(crate) struct IntegerEncoder {
    i: usize,
    mask: u8,
    pre: u8,
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
    pub(crate) fn new(i: usize, mask: u8, pre: u8) -> Self {
        Self {
            i,
            mask,
            pre,
            state: IntegerEncodeState::First,
        }
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

#[cfg(test)]
mod ut_integer {
    use crate::h2::hpack::integer::{IntegerDecoder, IntegerEncoder};

    /// UT test cases for `IntegerDecoder`.
    ///
    /// # Brief
    /// 1. Creates an `IntegerDecoder`.
    /// 2. Calls `IntegerDecoder::first_byte()` an
    ///    `IntegerDecoder::next_byte()`,
    /// passing in the specified parameters.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_integer_decode() {
        rfc7541_test_cases();

        macro_rules! integer_test_case {
            ($fb: literal, $mask: literal => $fb_res: expr) => {
                match IntegerDecoder::first_byte($fb, $mask) {
                    Ok(idx) => assert_eq!(idx, $fb_res),
                    _ => panic!("IntegerDecoder::first_byte() failed!"),
                }
            };
            ($fb: literal, $mask: literal $(, $nb: literal => $nb_res: expr)* $(,)?) => {
                match IntegerDecoder::first_byte($fb, $mask) {
                    Err(mut int) => {
                        $(match int.next_byte($nb) {
                            Ok(v) => assert_eq!(v, $nb_res),
                            _ => panic!("IntegerDecoder::next_byte() failed!"),
                        })*
                    }
                    _ => panic!("IntegerDecoder::first_byte() failed!"),
                }
            };
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.1.1. Example 1: Encoding 10 Using a 5-Bit Prefix
            integer_test_case!(0x0a, 0x1f => 10);

            // C.1.2.  Example 2: Encoding 1337 Using a 5-Bit Prefix
            integer_test_case!(
                0x1f, 0x1f,
                0x9a => None,
                0x0a => Some(1337),
            );

            // C.1.3.  Example 3: Encoding 42 Starting at an Octet Boundary
            integer_test_case!(0x2a, 0xff => 42);
        }
    }

    /// UT test cases for `IntegerEncoder`.
    ///
    /// # Brief
    /// 1. Creates an `IntegerEncoder`.
    /// 2. Calls `IntegerEncoder::first_byte()` and
    ///    `IntegerEncoder::next_byte()`,
    /// passing in the specified parameters.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_integer_encode() {
        rfc7541_test_cases();

        macro_rules! integer_test_case {
            ($int: expr, $mask: expr, $pre: expr $(, $byte: expr)* $(,)? ) => {
                let mut integer = IntegerEncoder::new($int, $mask, $pre);
                $(
                    assert_eq!(integer.next_byte(), Some($byte));
                )*
                assert_eq!(integer.next_byte(), None);
            }
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.1.1. Example 1: Encoding 10 Using a 5-Bit Prefix
            integer_test_case!(10, 0x1f, 0x00, 0x0a);

            // C.1.2. Example 2: Encoding 1337 Using a 5-Bit Prefix
            integer_test_case!(1337, 0x1f, 0x00, 0x1f, 0x9a, 0x0a);

            // C.1.3. Example 3: Encoding 42 Starting at an Octet Boundary
            integer_test_case!(42, 0xff, 0x00, 0x2a);
        }
    }
}
