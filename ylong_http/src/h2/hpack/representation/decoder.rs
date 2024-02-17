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

use core::cmp::Ordering;

use crate::h2::error::ErrorCode;
use crate::h2::hpack::integer::IntegerDecoder;
use crate::h2::hpack::representation::{Name, PrefixBit, Representation};
use crate::h2::H2Error;
use crate::huffman::HuffmanDecoder;

/// Decoder implementation for decoding representation. Every time users call
/// `decode`, the `ReprDecoder` will try to decode a `Repr`. If `buf` has been
/// fully decoded and users continue to call `decode`, it will return `None`.
/// Users need to save intermediate results in time.
pub(crate) struct ReprDecoder<'a> {
    /// `buf` represents the byte stream to be decoded.
    buf: &'a [u8],
    /// `state` represents the remaining state after the last call to `decode`.
    state: Option<ReprDecodeState>,
}

impl<'a> ReprDecoder<'a> {
    /// Creates a new `ReprDecoder` whose `state` is `None`.
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, state: None }
    }

    /// Loads state from a holder.
    pub(crate) fn load(&mut self, holder: &mut ReprDecStateHolder) {
        self.state = holder.state.take();
    }

    /// Decodes `self.buf`. Every time users call `decode`, it will try to
    /// decode a `Representation`.
    pub(crate) fn decode(&mut self) -> Result<Option<Representation>, H2Error> {
        // If buf is empty, leave the state unchanged.
        if self.buf.is_empty() {
            return Ok(None);
        }

        // If `self.state` is `None`, start decoding from the `Index` state.
        match self
            .state
            .take()
            .unwrap_or_else(|| ReprDecodeState::Index(Index::new()))
            .decode(&mut self.buf)
        {
            // If `buf` is not enough to continue decoding a complete
            // `Representation`, `Ok(None)` will be returned. Users need to call
            // `save` to save the current state to a `ReprDecStateHolder`.
            DecResult::NeedMore(state) => {
                self.state = Some(state);
                Ok(None)
            }
            // If a `Representation` is decoded, return it.
            DecResult::Decoded(repr) => Ok(Some(repr)),
            DecResult::Error(error) => Err(error),
        }
    }

    /// Saves current state to a `ReprDecStateHolder`.
    pub(crate) fn save(self, holder: &mut ReprDecStateHolder) {
        holder.state = self.state
    }
}

/// `ReprDecStateHolder` is used to hold the intermediate `DecodeState`.
pub(crate) struct ReprDecStateHolder {
    state: Option<ReprDecodeState>,
}

impl ReprDecStateHolder {
    pub(crate) fn new() -> Self {
        Self { state: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.state.is_none()
    }
}

macro_rules! state_def {
    ($name: ident, $decoded: ty, $($state: ident),* $(,)?) => {
        pub(crate) enum $name {
            $(
                $state($state),
            )*
        }

        impl $name {
            fn decode(self, buf: &mut &[u8]) -> DecResult<$decoded, $name> {
                match self {
                    $(
                        Self::$state(state) => state.decode(buf),
                    )*
                }
            }
        }

        $(
            impl From<$state> for $name {
                fn from(s: $state) -> Self {
                    Self::$state(s)
                }
            }
        )*
    }
}

// `Repr` decoding state diagram:
//
//                    ┌ `Index` ─ `IndexInner` ┬ `FirstByte`
//                    │                        └ `TrailingBytes`
//                    │
// `ReprDecodeState`  ┼ `NameString`  ┐                 ┌ `LengthFirstByte`
//                    │               ├ `LiteralString` ┼ `LengthTrailingBytes`
//                    └ `ValueString` ┘                 ├ `AsciiStringBytes`
//                                                      └ `HuffmanStringBytes`

// `ReprDecodeState` contains 3 states: `Index`, `NameString` and `ValueString`.
state_def!(
    ReprDecodeState,
    Representation,
    Index,
    NameString,
    ValueString
);

// `IndexInner` contains 2 states: `FirstByte` and `TrailingBytes`.
state_def!(IndexInner, (PrefixBit, usize), FirstByte, TrailingBytes);

// `LiteralString` contains 4 states: `LengthFirstByte`, `LengthTrailingBytes`,
// `AsciiStringBytes` and `HuffmanStringBytes`
state_def!(
    LiteralString,
    Vec<u8>,
    LengthFirstByte,
    LengthTrailingBytes,
    AsciiStringBytes,
    HuffmanStringBytes,
);

/// `Index` is responsible for decoding the starting index part.
pub(crate) struct Index {
    inner: IndexInner,
}

impl Index {
    fn new() -> Self {
        Self::from_inner(FirstByte.into())
    }

    fn from_inner(inner: IndexInner) -> Self {
        Self { inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match self.inner.decode(buf) {
            // RFC7541-6.1: The index value of 0 is not used. It MUST be treated
            // as a decoding error if found in an indexed header field representation.
            DecResult::Decoded((PrefixBit::INDEXED, 0)) => {
                H2Error::ConnectionError(ErrorCode::CompressionError).into()
            }
            DecResult::Decoded((PrefixBit::INDEXED, index)) => {
                DecResult::Decoded(Representation::Indexed { index })
            }
            DecResult::Decoded((PrefixBit::SIZE_UPDATE, max_size)) => {
                DecResult::Decoded(Representation::SizeUpdate { max_size })
            }
            DecResult::Decoded((repr, 0)) => NameString::new(repr).decode(buf),
            DecResult::Decoded((repr, index)) => {
                ValueString::new(repr, Name::Index(index)).decode(buf)
            }
            DecResult::NeedMore(inner) => DecResult::NeedMore(Index::from_inner(inner).into()),
            DecResult::Error(e) => e.into(),
        }
    }
}

/// `NameString` is responsible for decoding the name string part.
pub(crate) struct NameString {
    repr: PrefixBit,
    inner: LiteralString,
}

impl NameString {
    fn new(repr: PrefixBit) -> Self {
        Self::from_inner(repr, LengthFirstByte.into())
    }

    fn from_inner(repr: PrefixBit, inner: LiteralString) -> Self {
        Self { repr, inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded(octets) => {
                ValueString::new(self.repr, Name::Literal(octets)).decode(buf)
            }
            DecResult::NeedMore(inner) => {
                DecResult::NeedMore(Self::from_inner(self.repr, inner).into())
            }
            DecResult::Error(e) => e.into(),
        }
    }
}

/// `ValueString` is responsible for decoding the value string part.
pub(crate) struct ValueString {
    repr: PrefixBit,
    name: Name,
    inner: LiteralString,
}

impl ValueString {
    fn new(repr: PrefixBit, name: Name) -> Self {
        Self::from_inner(repr, name, LengthFirstByte.into())
    }

    fn from_inner(repr: PrefixBit, name: Name, inner: LiteralString) -> Self {
        Self { repr, name, inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match (self.repr, self.inner.decode(buf)) {
            (PrefixBit::LITERAL_WITH_INDEXING, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralWithIndexing {
                    name: self.name,
                    value,
                })
            }
            (PrefixBit::LITERAL_WITHOUT_INDEXING, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralWithoutIndexing {
                    name: self.name,
                    value,
                })
            }
            (_, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralNeverIndexed {
                    name: self.name,
                    value,
                })
            }
            (_, DecResult::NeedMore(inner)) => {
                DecResult::NeedMore(Self::from_inner(self.repr, self.name, inner).into())
            }
            (_, DecResult::Error(e)) => e.into(),
        }
    }
}

/// `FirstByte` is responsible for decoding the first byte of the index of
/// `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +--------+----------------------+
/// | Prefix |         Index        |
/// +--------+----------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct FirstByte;

impl FirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<(PrefixBit, usize), IndexInner> {
        // If `buf` has been completely decoded here, return the current state.
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }

        // Decodes `Prefix`.
        let byte = buf[0];
        let repr = PrefixBit::from_u8(byte);
        let mask = repr.prefix_index_mask();

        // Moves the pointer of `buf` backward.
        *buf = &buf[1..];
        match IntegerDecoder::first_byte(byte, mask.0) {
            Ok(idx) => DecResult::Decoded((repr, idx)),
            Err(int) => TrailingBytes::new(repr, int).decode(buf),
        }
    }
}

/// `TrailingBytes` is responsible for decoding the trailing bytes of the index
/// of `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +--------+----------------------+
/// | Prefix |         Index        |
/// +---+----+----------------------+
/// | 1 |          Index            |
/// +---+----+----------------------+
/// |             ...               |
/// +---+----+----------------------+
/// | 0 |          Index            |
/// +---+----+----------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct TrailingBytes {
    repr: PrefixBit,
    index: IntegerDecoder,
}

impl TrailingBytes {
    fn new(repr: PrefixBit, index: IntegerDecoder) -> Self {
        Self { repr, index }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<(PrefixBit, usize), IndexInner> {
        loop {
            // If `buf` has been completely decoded here, return the current state.
            if buf.is_empty() {
                return DecResult::NeedMore(self.into());
            }

            let byte = buf[0];
            *buf = &buf[1..];
            // Updates trailing bytes until we get the index.
            match self.index.next_byte(byte) {
                Ok(None) => {}
                Ok(Some(index)) => return DecResult::Decoded((self.repr, index)),
                Err(e) => return e.into(),
            }
        }
    }
}

/// `LengthFirstByte` is responsible for decoding the first byte of the
/// length in `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +---+---------------------------+
/// | H |           Length          |
/// +---+---------------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct LengthFirstByte;

impl LengthFirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }

        let byte = buf[0];
        *buf = &buf[1..];
        match (
            IntegerDecoder::first_byte(byte, 0x7f),
            (byte & 0x80) == 0x80,
        ) {
            (Ok(len), true) => HuffmanStringBytes::new(len).decode(buf),
            (Ok(len), false) => AsciiStringBytes::new(len).decode(buf),
            (Err(int), huffman) => LengthTrailingBytes::new(huffman, int).decode(buf),
        }
    }
}

/// `LengthTrailingBytes` is responsible for decoding the trailing bytes of the
/// length in `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +---+---------------------------+
/// | H |           Length          |
/// +---+---------------------------+
/// | 1 |           Length          |
/// +---+----+----------------------+
/// |             ...               |
/// +---+----+----------------------+
/// | 0 |           Length          |
/// +---+----+----------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct LengthTrailingBytes {
    is_huffman: bool,
    length: IntegerDecoder,
}

impl LengthTrailingBytes {
    fn new(is_huffman: bool, length: IntegerDecoder) -> Self {
        Self { is_huffman, length }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        loop {
            if buf.is_empty() {
                return DecResult::NeedMore(self.into());
            }

            let byte = buf[0];
            *buf = &buf[1..];
            match (self.length.next_byte(byte), self.is_huffman) {
                (Ok(None), _) => {}
                (Err(e), _) => return e.into(),
                (Ok(Some(length)), true) => return HuffmanStringBytes::new(length).decode(buf),
                (Ok(Some(length)), false) => return AsciiStringBytes::new(length).decode(buf),
            }
        }
    }
}

/// `AsciiStringBytes` is responsible for decoding the ascii string of the
/// literal octets of `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +-------------------------------+
/// |            String             |
/// +-------------------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct AsciiStringBytes {
    octets: Vec<u8>,
    length: usize,
}

impl AsciiStringBytes {
    fn new(length: usize) -> Self {
        Self {
            octets: Vec::new(),
            length,
        }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        match (buf.len() + self.octets.len()).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.octets.len();
                self.octets.extend_from_slice(&buf[..pos]);
                *buf = &buf[pos..];
                DecResult::Decoded(self.octets)
            }
            Ordering::Less => {
                self.octets.extend_from_slice(buf);
                *buf = &buf[buf.len()..];
                DecResult::NeedMore(self.into())
            }
        }
    }
}

/// `HuffmanStringBytes` is responsible for decoding the huffman string of the
/// literal octets of `Representation`.
///
/// # Binary Format
/// ```text
/// Bytes list:
/// +-------------------------------+
/// |            String             |
/// +-------------------------------+
/// |      (trailing bytes)...      |
/// +--------+----------------------+
/// ```
pub(crate) struct HuffmanStringBytes {
    huffman: HuffmanDecoder,
    read: usize,
    length: usize,
}

impl HuffmanStringBytes {
    fn new(length: usize) -> Self {
        Self {
            huffman: HuffmanDecoder::new(),
            read: 0,
            length,
        }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        match (buf.len() + self.read).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.read;
                if self.huffman.decode(&buf[..pos]).is_err() {
                    return H2Error::ConnectionError(ErrorCode::CompressionError).into();
                }
                *buf = &buf[pos..];
                match self.huffman.finish() {
                    Ok(vec) => DecResult::Decoded(vec),
                    Err(_) => H2Error::ConnectionError(ErrorCode::CompressionError).into(),
                }
            }
            Ordering::Less => {
                if self.huffman.decode(buf).is_err() {
                    return H2Error::ConnectionError(ErrorCode::CompressionError).into();
                }
                self.read += buf.len();
                *buf = &buf[buf.len()..];
                DecResult::NeedMore(self.into())
            }
        }
    }
}

/// Finish state, generate `Representation` by name and value.
pub(crate) struct Finish {
    repr: PrefixBit,
    name: Name,
    value: Vec<u8>,
}

impl Finish {
    fn new(repr: PrefixBit, name: Name, value: Vec<u8>) -> Self {
        Self { repr, name, value }
    }

    fn decode(self) -> DecResult<Representation, ReprDecodeState> {
        DecResult::Decoded(match self.repr {
            PrefixBit::LITERAL_WITH_INDEXING => Representation::LiteralWithIndexing {
                name: self.name,
                value: self.value,
            },
            PrefixBit::LITERAL_WITHOUT_INDEXING => Representation::LiteralWithoutIndexing {
                name: self.name,
                value: self.value,
            },
            _ => Representation::LiteralNeverIndexed {
                name: self.name,
                value: self.value,
            },
        })
    }
}

/// Decoder's possible returns during the decoding process.
enum DecResult<D, S> {
    /// Decoder has got a `D`. Users can continue to call `encode` to try to
    /// get the next `D`.
    Decoded(D),

    /// Decoder needs more bytes to decode to get a `D`. Returns the current
    /// decoding state `S`.
    NeedMore(S),

    /// Errors that may occur when decoding.
    Error(H2Error),
}

impl<D, S> From<H2Error> for DecResult<D, S> {
    fn from(e: H2Error) -> Self {
        DecResult::Error(e)
    }
}

#[cfg(test)]
mod ut_repr_decoder {
    use super::*;
    use crate::util::test_util::decode;

    /// UT test cases for `ReprDecoder`.
    ///
    /// # Brief
    /// 1. Creates a `ReprDecoder`.
    /// 2. Calls `ReprDecoder::decode()` function, passing in the specified
    /// parameters.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_repr_decoder() {
        rfc7541_test_cases();

        macro_rules! inner_test_case {
            ($decoder: expr, $pat: ident => $name: expr) => {
                match $decoder.decode() {
                    Ok(Some(Representation::$pat { index })) => assert_eq!($name, index),
                    _ => panic!("ReprDecoder::decode() failed!"),
                }
            };
            ($decoder: expr, $pat: ident, $kind: ident => $name: expr, $value: expr) => {
                match $decoder.decode() {
                    Ok(Some(Representation::$pat {
                        name: Name::$kind(n),
                        value: v,
                    })) => {
                        assert_eq!($name, n);
                        assert_eq!($value, v);
                    }
                    _ => panic!("ReprDecoder::decode() failed!"),
                }
            };
        }

        macro_rules! repr_test_case {
            ($octets: literal, $({ $pat: ident $(, $kind: ident)? => $first: expr $(, $second: expr)?} $(,)?)*) => {
                let slice = decode($octets).unwrap();
                let mut decoder = ReprDecoder::new(&slice);
                $(
                    inner_test_case!(decoder, $pat $(, $kind)? => $first $(, $second)?);
                )*
            }
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.2.1.  Literal Header Field with Indexing
            repr_test_case!(
                "400a637573746f6d2d6b65790d637573746f6d2d686561646572",
                { LiteralWithIndexing, Literal => b"custom-key".to_vec(), b"custom-header".to_vec() }
            );

            // C.2.2.  Literal Header Field without Indexing
            repr_test_case!(
                "040c2f73616d706c652f70617468",
                { LiteralWithoutIndexing, Index => 4, b"/sample/path".to_vec() }
            );

            // C.2.3.  Literal Header Field Never Indexed
            repr_test_case!(
                "100870617373776f726406736563726574",
                { LiteralNeverIndexed, Literal => b"password".to_vec(), b"secret".to_vec() }
            );

            // C.2.4.  Indexed Header Field
            repr_test_case!(
                "82",
                { Indexed => 2 }
            );

            // C.3.1.  First Request
            repr_test_case!(
                "828684410f7777772e6578616d706c652e636f6d",
                { Indexed => 2 },
                { Indexed => 6 },
                { Indexed => 4 },
                { LiteralWithIndexing, Index => 1, b"www.example.com".to_vec() }
            );

            // C.3.2.  Second Request
            repr_test_case!(
                "828684be58086e6f2d6361636865",
                { Indexed => 2 },
                { Indexed => 6 },
                { Indexed => 4 },
                { Indexed => 62 },
                { LiteralWithIndexing, Index => 24, b"no-cache".to_vec() }
            );

            // C.3.3.  Third Request
            repr_test_case!(
                "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
                { Indexed => 2 },
                { Indexed => 7 },
                { Indexed => 5 },
                { Indexed => 63 },
                { LiteralWithIndexing, Literal => b"custom-key".to_vec(), b"custom-value".to_vec() }
            );

            // C.4.1.  First Request
            repr_test_case!(
                "828684418cf1e3c2e5f23a6ba0ab90f4ff",
                { Indexed => 2 },
                { Indexed => 6 },
                { Indexed => 4 },
                { LiteralWithIndexing, Index => 1, b"www.example.com".to_vec() }
            );

            // C.4.2.  Second Request
            repr_test_case!(
                "828684be5886a8eb10649cbf",
                { Indexed => 2 },
                { Indexed => 6 },
                { Indexed => 4 },
                { Indexed => 62 },
                { LiteralWithIndexing, Index => 24, b"no-cache".to_vec() }
            );

            // C.4.3.  Third Request
            repr_test_case!(
                "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
                { Indexed => 2 },
                { Indexed => 7 },
                { Indexed => 5 },
                { Indexed => 63 },
                { LiteralWithIndexing, Literal => b"custom-key".to_vec(), b"custom-value".to_vec() }
            );

            // C.5.1.  First Response
            repr_test_case!(
                "4803333032580770726976617465611d\
                4d6f6e2c203231204f63742032303133\
                2032303a31333a323120474d546e1768\
                747470733a2f2f7777772e6578616d70\
                6c652e636f6d",
                { LiteralWithIndexing, Index => 8, b"302".to_vec() },
                { LiteralWithIndexing, Index => 24, b"private".to_vec() },
                { LiteralWithIndexing, Index => 33, b"Mon, 21 Oct 2013 20:13:21 GMT".to_vec() },
                { LiteralWithIndexing, Index => 46, b"https://www.example.com".to_vec() }
            );

            // C.5.2.  Second Response
            repr_test_case!(
                "4803333037c1c0bf",
                { LiteralWithIndexing, Index => 8, b"307".to_vec() },
                { Indexed => 65 },
                { Indexed => 64 },
                { Indexed => 63 }
            );

            // C.5.3.  Third Response
            repr_test_case!(
                "88c1611d4d6f6e2c203231204f637420\
                323031332032303a31333a323220474d\
                54c05a04677a69707738666f6f3d4153\
                444a4b48514b425a584f5157454f5049\
                5541585157454f49553b206d61782d61\
                67653d333630303b2076657273696f6e\
                3d31",
                { Indexed => 8 },
                { Indexed => 65 },
                { LiteralWithIndexing, Index => 33, b"Mon, 21 Oct 2013 20:13:22 GMT".to_vec() },
                { Indexed => 64 },
                { LiteralWithIndexing, Index => 26, b"gzip".to_vec() },
                { LiteralWithIndexing, Index => 55, b"foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1".to_vec() }
            );

            // C.6.1.  First Response
            repr_test_case!(
                "488264025885aec3771a4b6196d07abe\
                941054d444a8200595040b8166e082a6\
                2d1bff6e919d29ad171863c78f0b97c8\
                e9ae82ae43d3",
                { LiteralWithIndexing, Index => 8, b"302".to_vec() },
                { LiteralWithIndexing, Index => 24, b"private".to_vec() },
                { LiteralWithIndexing, Index => 33, b"Mon, 21 Oct 2013 20:13:21 GMT".to_vec() },
                { LiteralWithIndexing, Index => 46, b"https://www.example.com".to_vec() }
            );

            // C.6.2.  Second Response
            repr_test_case!(
                "4883640effc1c0bf",
                { LiteralWithIndexing, Index => 8, b"307".to_vec() },
                { Indexed => 65 },
                { Indexed => 64 },
                { Indexed => 63 }
            );

            // C.6.3.  Third Response
            repr_test_case!(
                "88c16196d07abe941054d444a8200595\
                040b8166e084a62d1bffc05a839bd9ab\
                77ad94e7821dd7f2e6c7b335dfdfcd5b\
                3960d5af27087f3672c1ab270fb5291f\
                9587316065c003ed4ee5b1063d5007",
                { Indexed => 8 },
                { Indexed => 65 },
                { LiteralWithIndexing, Index => 33, b"Mon, 21 Oct 2013 20:13:22 GMT".to_vec() },
                { Indexed => 64 },
                { LiteralWithIndexing, Index => 26, b"gzip".to_vec() },
                { LiteralWithIndexing, Index => 55, b"foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1".to_vec() }
            );
        }
    }

    /// UT test cases for `Finish::decode`.
    ///
    /// # Brief
    /// 1. Creates a `Finish` instance.
    /// 2. Calls `Finish::decode` to decode.
    /// 3. Checks the results.
    #[test]
    fn ut_finish_decode() {
        // Case 1: Testing LiteralWithIndexing
        let name = Name::Literal("test_name".as_bytes().to_vec());
        let value = vec![0u8, 1u8, 2u8];
        let finish = Finish::new(PrefixBit::LITERAL_WITH_INDEXING, name, value);
        match finish.decode() {
            DecResult::Decoded(repr) => match repr {
                Representation::LiteralWithIndexing { name: _, value: _ } => {}
                _ => panic!("Expected LiteralWithIndexing"),
            },
            _ => panic!("Decode failed"),
        }

        // Case 2: Testing LiteralWithoutIndexing
        let name = Name::Literal("test_name".as_bytes().to_vec());
        let value = vec![0u8, 1u8, 2u8];
        let finish = Finish::new(PrefixBit::LITERAL_WITHOUT_INDEXING, name, value);
        match finish.decode() {
            DecResult::Decoded(repr) => match repr {
                Representation::LiteralWithoutIndexing { name: _, value: _ } => {}
                _ => panic!("Expected LiteralWithoutIndexing"),
            },
            _ => panic!("Decode failed"),
        }

        // Case 3: Testing LiteralNeverIndexed
        let name = Name::Literal("test_name".as_bytes().to_vec());
        let value = vec![0u8, 1u8, 2u8];
        let finish = Finish::new(PrefixBit::LITERAL_NEVER_INDEXED, name, value);
        match finish.decode() {
            DecResult::Decoded(repr) => match repr {
                Representation::LiteralNeverIndexed { name: _, value: _ } => {}
                _ => panic!("Expected LiteralNeverIndexed"),
            },
            _ => panic!("Decode failed"),
        }

        // Case 4: Testing Indexed
        let name = Name::Literal("test_name".as_bytes().to_vec());
        let value = vec![0u8, 1u8, 2u8];
        let finish = Finish::new(PrefixBit::INDEXED, name, value);
        match finish.decode() {
            DecResult::Decoded(repr) => match repr {
                Representation::LiteralNeverIndexed { name: _, value: _ } => {}
                _ => panic!("Expected LiteralNeverIndexed for PrefixBit::INDEXED"),
            },
            _ => panic!("Decode failed"),
        }

        // Case 5: Testing SizeUpdate
        let name = Name::Literal("test_name".as_bytes().to_vec());
        let value = vec![0u8, 1u8, 2u8];
        let finish = Finish::new(PrefixBit::SIZE_UPDATE, name, value);
        match finish.decode() {
            DecResult::Decoded(repr) => match repr {
                Representation::LiteralNeverIndexed { name: _, value: _ } => {}
                _ => panic!("Expected LiteralNeverIndexed for PrefixBit::SIZE_UPDATE"),
            },
            _ => panic!("Decode failed"),
        }
    }
}
