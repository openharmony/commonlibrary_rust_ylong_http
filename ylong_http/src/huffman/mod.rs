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

//! [Huffman coding] implementation of the HTTP/2 protocol.
//!
//! [Huffman Coding]: https://en.wikipedia.org/wiki/Huffman_coding
//!
//! # What is Huffman coding
//! In computer science and information theory, a `Huffman code` is a particular
//! type of optimal prefix code that is commonly used for lossless data
//! compression. The process of finding or using such a code proceeds by means
//! of `Huffman coding`, an algorithm developed by David A. Huffman while he was
//! a Sc.D. student at MIT, and published in the 1952 paper "A Method for the
//! Construction of Minimum-Redundancy Codes".
//!
//! # Huffman code in Http/2
//! There is a table of Huffman code in `RFC7541`. This [Huffman code] was
//! generated from statistics obtained on a large sample of HTTP headers. It is
//! a canonical Huffman code with some tweaking to ensure that no symbol has a
//! unique code length.
//!
//! [Huffman Code]: https://www.rfc-editor.org/rfc/rfc7541.html#ref-HUFFMAN

// TODO: Introduction of `Huffman code in Http/3`.

mod consts;

use core::cmp::Ordering;

use consts::{HUFFMAN_DECODE, HUFFMAN_ENCODE};

/// Converts a string to a Huffman code, and then put it into the
/// specified `Vec<u8>`.
pub(crate) fn huffman_encode(src: &[u8], dst: &mut Vec<u8>) {
    // We use `state` to hold temporary encoding state.
    // We use `unfilled` to represent the remaining number of bits that is not
    // filled. Each time any bytes are encoded, we will store the result bits
    // in `state`.
    //
    // When `state` is not full, we add the result bits to `Unfilled`.
    // `state`:
    // +----------+----------+----------------------------+
    // | Result A | Result B |          Unfilled          |
    // +----------+----------+----------------------------+
    // |<-------------------  64 bits  ------------------->
    //
    // When the length of the result bits is greater than the length of `Unfilled`,
    // we will truncate it.
    // `state`:
    // +---------------------+----------------------------+
    // |                     |     A part of Result C     | -> Output it.
    // +---------------------+----------------------------+
    // |<--------------  full 64 bits  ------------------->
    //
    // Final `state`:
    // +--------------------------------+-----------------+
    // | The remaining part of Result C |     Unfilled    |
    // +--------------------------------+-----------------+

    let mut state = 0u64;
    // The initial value of `unfilled` is equal to the number of bits in the
    // `state`.
    let mut unfilled = 64;

    for byte in src.iter() {
        let (nbits, code) = HUFFMAN_ENCODE[*byte as usize];
        match unfilled.cmp(&nbits) {
            Ordering::Greater => {
                state |= code << (unfilled - nbits);
                unfilled -= nbits;
            }
            Ordering::Equal => {
                state |= code;
                dst.extend_from_slice(&state.to_be_bytes());
                state = 0;
                unfilled = 64;
            }
            // We rotate the `code` to the right, and we will get `rotate`.
            // `rotate`:
            // +---------+-----------------+----------+
            // | Parts A |                 |  Parts B |
            // +---------+-----------------+----------+
            // `mask`:
            // +---------+-----------------+----------+
            // | 000...0 |         111...1            |
            // +---------+-----------------+----------+
            // `rotate` & mask => Parts B
            // `rotate` & !mask => Parts A
            Ordering::Less => {
                let rotate = code.rotate_right((nbits - unfilled) as u32);
                let mask = u64::MAX >> (64 - unfilled);
                state |= rotate & mask;
                dst.extend_from_slice(&state.to_be_bytes());
                state = rotate & !mask;
                unfilled = 64 - (nbits - unfilled);
            }
        }
    }

    // At the end of character decoding, if the last byte is not completely
    // filled, it needs to be filled with `0b1`.
    if unfilled != 64 {
        state |= u64::MAX >> (64 - unfilled);
        let bytes = &state.to_be_bytes();
        // Here we only need to output the filled bytes, not all the `state`.
        let len = (8 - (unfilled >> 3)) as usize;
        dst.extend_from_slice(&bytes.as_slice()[..len]);
    }
}

/// Converts a Huffman code into a literal string at one time, and then put it
/// into the specified `Vec<u8>`.
///
/// The algorithm comes from crate [h2].
///
/// [h2]: https://crates.io/crates/h2
pub(crate) fn huffman_decode(src: &[u8], dst: &mut Vec<u8>) -> Result<(), HuffmanDecodeError> {
    // We use a state machine to parse Huffman code.
    // We have a `HUFFMAN_DECODE` table, which contains three elements:
    // `State`, `Decoded Byte`, and `Flags`.
    //
    // `State` represents the current state. It can be considered as the value
    // composed of the remaining unparsed bits after the last parsing is completed.
    //
    // `Decoded Byte` represents the decoded character when the `Decoded` bit
    // of `Flags` is set to `0b1`.
    //
    // `Flags` contains three bits: `MAYBE_EOS`(0x1), `DECODED`(0x2), `ERROR`(0x4).
    //
    // When `MAYBE_EOS` is set, it means that the current character may be the
    // end of the literal string.
    //
    // When `DECODED` is set, it means that the character at `Decoded Bytes` is
    // the current decoded character.
    //
    // When `ERROR` is set, it means that there is a wrong Huffman code in the
    // byte sequence.
    //
    // We use a variable called `state` to hold the current state. Its initial
    // value is 0. Every time we take out 4 bits from the byte sequence that
    // needs to be parsed and look it up in the `HUFFMAN_DECODE` table. Then we
    // will get `state`, `byte` and `flags`. We can judge the current state
    // through `flags` and choose to continue decoding or return an error.

    let (state, flags) = huffman_decode_inner(src, dst, 0, 0)?;

    // The decoding operation succeeds in the following two cases:
    // 1. `state` is 0. It means that all bits are parsed normally.
    // 2. `state` is not 0, but `maybe_eos` is true. It means that not all bits
    // are parsed and the last byte can be the terminator. The remaining bits are
    // allowed because a part of the padding is done when http/2 performs
    // Huffman encoding.
    if state != 0 && (flags & 0x1) == 0 {
        return Err(HuffmanDecodeError::InvalidHuffmanCode);
    }

    Ok(())
}

fn huffman_decode_inner(
    src: &[u8],
    dst: &mut Vec<u8>,
    state: u8,
    flags: u8,
) -> Result<(u8, u8), HuffmanDecodeError> {
    let (mut state, mut _result, mut flags) = (state, 0, flags);

    for byte in src.iter() {
        let left = byte >> 4;
        let right = byte & 0xf;

        (state, _result, flags) = HUFFMAN_DECODE[state as usize][left as usize];
        if (flags & 0x4) == 0x4 {
            return Err(HuffmanDecodeError::InvalidHuffmanCode);
        }
        if (flags & 0x2) == 0x2 {
            dst.push(_result);
        }

        (state, _result, flags) = HUFFMAN_DECODE[state as usize][right as usize];
        if (flags & 0x4) == 0x4 {
            return Err(HuffmanDecodeError::InvalidHuffmanCode);
        }
        if (flags & 0x2) == 0x2 {
            dst.push(_result);
        }
    }
    Ok((state, flags))
}

/// Converts a Huffman code into a literal string, and then put it into the
/// specified `Vec<u8>`. Users can split the string into multiple slices and
/// then pass them into `HuffmanDecoder` to get the result.
///
/// The algorithm comes from crate [h2].
///
/// [h2]: https://crates.io/crates/h2
pub(crate) struct HuffmanDecoder {
    state: u8,
    flags: u8,
    vec: Vec<u8>,
}

impl HuffmanDecoder {
    /// Creates a new, empty `HuffmanDecoder`.
    pub(crate) fn new() -> Self {
        Self {
            state: 0,
            flags: 0,
            vec: Vec::new(),
        }
    }

    /// Decodes input string. Stop when the `src` is used up.
    pub(crate) fn decode(&mut self, src: &[u8]) -> Result<(), HuffmanDecodeError> {
        (self.state, self.flags) =
            huffman_decode_inner(src, &mut self.vec, self.state, self.flags)?;
        Ok(())
    }

    /// Finishes decoding and get the decoded result.
    pub(crate) fn finish(self) -> Result<Vec<u8>, HuffmanDecodeError> {
        if self.state != 0 && (self.flags & 0x1) == 0 {
            return Err(HuffmanDecodeError::InvalidHuffmanCode);
        }
        Ok(self.vec)
    }
}

/// Possible errors in Huffman decoding operations.
#[derive(Debug)]
pub(crate) enum HuffmanDecodeError {
    InvalidHuffmanCode,
}

#[cfg(test)]
mod ut_huffman {
    use super::{huffman_decode, huffman_encode, HuffmanDecoder};
    use crate::util::test_util::decode;

    /// UT test cases for `huffman_encode`.
    ///
    /// # Brief
    /// 1. Calls `huffman_encode` function, passing in the specified parameters.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_huffman_encode() {
        rfc7541_test_cases();

        macro_rules! huffman_test_case {
            ($ctn: expr, $res: expr $(,)?) => {
                let mut vec = Vec::new();
                huffman_encode($ctn.as_bytes(), &mut vec);
                assert_eq!(vec, decode($res).unwrap())
            };
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.4.1 First Request
            huffman_test_case!("www.example.com", "f1e3c2e5f23a6ba0ab90f4ff");

            // C.4.2 Second Request
            huffman_test_case!("no-cache", "a8eb10649cbf");

            // C.4.3 Third Request
            huffman_test_case!("custom-value", "25a849e95bb8e8b4bf");

            // C.6.1 First Response
            huffman_test_case!("302", "6402");
            huffman_test_case!("private", "aec3771a4b");
            huffman_test_case!(
                "Mon, 21 Oct 2013 20:13:21 GMT",
                "d07abe941054d444a8200595040b8166e082a62d1bff"
            );
            huffman_test_case!(
                "https://www.example.com",
                "9d29ad171863c78f0b97c8e9ae82ae43d3"
            );

            // C.6.2 Second Response
            huffman_test_case!("307", "640eff");

            // C.6.3 Third Response
            huffman_test_case!("gzip", "9bd9ab");
            huffman_test_case!(
                "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1",
                "94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007",
            );
        }
    }

    /// UT test cases for `huffman_decode`.
    ///
    /// # Brief
    /// 1. Calls `huffman_decode` function, passing in the specified parameters.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_huffman_decode() {
        rfc7541_test_cases();

        macro_rules! huffman_test_case {
            ($ctn: expr, $res: expr $(,)?) => {
                let mut vec = Vec::new();
                huffman_decode(decode($ctn).unwrap().as_slice(), &mut vec).unwrap();
                assert_eq!(vec.as_slice(), $res.as_bytes())
            };
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.4.1 First Request
            huffman_test_case!("f1e3c2e5f23a6ba0ab90f4ff", "www.example.com");

            // C.4.2 Second Request
            huffman_test_case!("a8eb10649cbf", "no-cache");

            // C.4.3 Third Request
            huffman_test_case!("25a849e95bb8e8b4bf", "custom-value");

            // C.6.1 First Response
            huffman_test_case!("6402", "302");
            huffman_test_case!("aec3771a4b", "private");
            huffman_test_case!(
                "d07abe941054d444a8200595040b8166e082a62d1bff",
                "Mon, 21 Oct 2013 20:13:21 GMT"
            );
            huffman_test_case!(
                "9d29ad171863c78f0b97c8e9ae82ae43d3",
                "https://www.example.com",
            );

            // C.6.2 Second Response
            huffman_test_case!("640eff", "307");

            // C.6.3 Third Response
            huffman_test_case!("9bd9ab", "gzip");
            huffman_test_case!(
                "94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007",
                "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"
            );
        }
    }

    /// UT test cases for `HuffmanDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `HuffmanDecoder`.
    /// 1. Calls `decode` and `finish` function, passing in the specified
    ///    parameters.
    /// 2. Checks if the test results are correct.
    #[test]
    fn ut_huffman_decoder() {
        rfc7541_test_cases();
        slices_test();

        macro_rules! huffman_test_case {
            ($content: expr, $result: expr) => {{
                let mut decoder = HuffmanDecoder::new();
                for cont in $content.as_slice().iter() {
                    let bytes = decode(cont).unwrap();
                    assert!(decoder.decode(&bytes).is_ok());
                }
                match decoder.finish() {
                    Ok(vec) => vec == $result.as_bytes(),
                    _ => panic!(),
                }
            }};
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.4.1 First Request
            huffman_test_case!(["f1e3c2e5f23a6ba0ab90f4ff"], "www.example.com");

            // C.4.2 Second Request
            huffman_test_case!(["a8eb10649cbf"], "no-cache");

            // C.4.3 Third Request
            huffman_test_case!(["25a849e95bb8e8b4bf"], "custom-value");

            // C.6.1 First Response
            huffman_test_case!(["6402"], "302");
            huffman_test_case!(["aec3771a4b"], "private");
            huffman_test_case!(
                ["d07abe941054d444a8200595040b8166e082a62d1bff"],
                "Mon, 21 Oct 2013 20:13:21 GMT"
            );
            huffman_test_case!(
                ["9d29ad171863c78f0b97c8e9ae82ae43d3"],
                "https://www.example.com"
            );

            // C.6.2 Second Response
            huffman_test_case!(["640eff"], "307");

            // C.6.3 Third Response
            huffman_test_case!(["9bd9ab"], "gzip");
            huffman_test_case!(
                ["94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007"],
                "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"
            );
        }

        /// The following test cases is for testing segmented byte slices.
        fn slices_test() {
            // Fragmentation
            huffman_test_case!(["a8", "eb", "10", "64", "9c", "bf"], "no-cache");

            // Fragmentation + Blank
            huffman_test_case!(
                ["", "", "", "", "a8", "", "eb", "10", "", "64", "9c", "", "bf", "", ""],
                "no-cache"
            );
        }
    }
}
