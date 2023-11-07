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

use crate::h3::qpack::error::ErrorCode::DecompressionFailed;
use crate::h3::qpack::error::{ErrorCode, H3errorQpack};
use crate::h3::qpack::format::decoder::DecResult::Error;
use crate::h3::qpack::integer::IntegerDecoder;
use crate::h3::qpack::{
    DeltaBase, EncoderInstPrefixBit, EncoderInstruction, MidBit, PrefixMask, ReprPrefixBit,
    Representation, RequireInsertCount,
};
use crate::huffman::HuffmanDecoder;
use std::cmp::Ordering;
use std::marker::PhantomData;

pub(crate) struct EncInstDecoder;

impl EncInstDecoder {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Decodes `buf`. Every time users call `decode`, it will try to
    /// decode a `EncoderInstruction`.
    /// # Example
//     ```no_run
//     use crate::h3::qpack::format::decoder::EncInstDecoder;
//     let mut decoder = EncInstDecoder::new();
//
//     ```
    pub(crate) fn decode(
        &mut self,
        buf: &[u8],
        inst_state: &mut Option<InstDecodeState>,
    ) -> Result<Option<(usize, EncoderInstruction)>, H3errorQpack> {
        if buf.is_empty() {
            return Ok(None);
        }

        match inst_state
            .take()
            .unwrap_or_else(|| InstDecodeState::EncInstIndex(EncInstIndex::new()))
            .decode(buf)
        {
            // If `buf` is not enough to continue decoding a complete
            // `Representation`, `Ok(None)` will be returned. Users need to call
            // `save` to save the current state to a `ReprDecStateHolder`.
            DecResult::NeedMore(state) => {
                println!("need more");
                *inst_state = Some(state);
                Ok(None)
            }
            DecResult::Decoded((buf_index, repr)) => Ok(Some((buf_index, repr))),

            DecResult::Error(error) => Err(error),
        }
    }
}

pub(crate) struct ReprDecoder;

impl ReprDecoder {
    /// Creates a new `ReprDecoder` whose `state` is `None`.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Decodes `self.buf`. Every time users call `decode`, it will try to
    /// decode a `Representation`.
    pub(crate) fn decode(
        &mut self,
        buf: &[u8],
        repr_state: &mut Option<ReprDecodeState>,
    ) -> Result<Option<(usize, Representation)>, H3errorQpack> {
        // If buf is empty, leave the state unchanged.
        let buf_len = buf.len();
        if buf.is_empty() {
            return Ok(None);
        }
        match repr_state
            .take()
            .unwrap_or_else(|| ReprDecodeState::FiledSectionPrefix(FiledSectionPrefix::new()))
            .decode(buf)
        {
            // If `buf` is not enough to continue decoding a complete
            // `Representation`, `Ok(None)` will be returned. Users need to call
            // `save` to save the current state to a `ReprDecStateHolder`.
            DecResult::NeedMore(state) => {
                println!("need more");
                *repr_state = Some(state);
                Ok(None)
            }
            DecResult::Decoded((buf_index, repr)) => {
                if buf_index >= buf_len {
                    return Ok(Some((buf_index, repr)));
                }
                *repr_state = Some(ReprDecodeState::ReprIndex(ReprIndex::new()));
                Ok(Some((buf_index, repr)))
            }

            DecResult::Error(error) => Err(error),
        }
    }
}
macro_rules! return_res {
    ($res: expr ,$buf_index: expr) => {
        if let DecResult::Decoded((cnt, repr)) = $res {
            return DecResult::Decoded((cnt + $buf_index, repr));
        } else {
            $res
        }
    };
}

macro_rules! state_def {
    ($name: ident, $decoded: ty, $($state: ident),* $(,)?) => {
        pub(crate) enum $name {
            $(
                $state($state),
            )*
        }

        impl $name {
            fn decode(self, buf: & [u8]) -> DecResult<$decoded, $name> {
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

state_def!(
    FSPInner,
    (usize, RequireInsertCount, bool, DeltaBase),
    FSPTwoIntergers,
);
state_def!(
    InstDecodeState,
    (usize, EncoderInstruction),
    EncInstIndex,
    InstValueString,
    InstNameAndValue,
);

state_def!(
    ReprDecodeState,
    (usize, Representation),
    FiledSectionPrefix,
    ReprIndex,
    ReprValueString,
    ReprNameAndValue,
);
state_def!(
    InstIndexInner,
    (usize, EncoderInstPrefixBit, MidBit, usize),
    InstFirstByte,
    InstTrailingBytes
);
state_def!(
    ReprIndexInner,
    (usize, ReprPrefixBit, MidBit, usize),
    ReprFirstByte,
    ReprTrailingBytes
);

state_def!(
    LiteralString,
    (usize, Vec<u8>),
    LengthFirstByte,
    LengthTrailingBytes,
    AsciiStringBytes,
    HuffmanStringBytes,
);

pub(crate) struct FiledSectionPrefix {
    inner: FSPInner,
}

impl FiledSectionPrefix {
    fn new() -> Self {
        Self::from_inner(FSPTwoIntergers.into())
    }
    fn from_inner(inner: FSPInner) -> Self {
        Self { inner }
    }

    fn decode(self, buf: &[u8]) -> DecResult<(usize, Representation), ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((buf_index, ric, signal, delta_base)) => DecResult::Decoded((
                buf_index,
                Representation::FieldSectionPrefix {
                    require_insert_count: ric,
                    signal,
                    delta_base,
                },
            )),
            DecResult::NeedMore(inner) => {
                DecResult::NeedMore(FiledSectionPrefix::from_inner(inner).into())
            }
            DecResult::Error(e) => e.into(),
        }
    }
}

pub(crate) struct EncInstIndex {
    inner: InstIndexInner,
}

impl EncInstIndex {
    fn new() -> Self {
        Self::from_inner(InstFirstByte.into())
    }
    fn from_inner(inner: InstIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &[u8]) -> DecResult<(usize, EncoderInstruction), InstDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((buf_index, EncoderInstPrefixBit::SETCAP, _, index)) => {
                DecResult::Decoded((buf_index, EncoderInstruction::SetCap { capacity: index }))
            }
            DecResult::Decoded((
                buf_index,
                EncoderInstPrefixBit::INSERTWITHINDEX,
                mid_bit,
                index,
            )) => {
                let res = InstValueString::new(
                    EncoderInstPrefixBit::INSERTWITHINDEX,
                    mid_bit,
                    Name::Index(index),
                )
                .decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            DecResult::Decoded((
                buf_index,
                EncoderInstPrefixBit::INSERTWITHLITERAL,
                mid_bit,
                namelen,
            )) => {
                let res = InstNameAndValue::new(
                    EncoderInstPrefixBit::INSERTWITHLITERAL,
                    mid_bit,
                    namelen,
                )
                .decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            DecResult::Decoded((remain_buf, EncoderInstPrefixBit::DUPLICATE, _, index)) => {
                DecResult::Decoded((remain_buf, EncoderInstruction::Duplicate { index }))
            }
            DecResult::NeedMore(inner) => {
                DecResult::NeedMore(EncInstIndex::from_inner(inner).into())
            }
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3errorQpack::ConnectionError(
                ErrorCode::DecompressionFailed,
            )),
        }
    }
}

pub(crate) struct ReprIndex {
    inner: ReprIndexInner,
}

impl ReprIndex {
    fn new() -> Self {
        Self::from_inner(ReprFirstByte.into())
    }
    fn from_inner(inner: ReprIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &[u8]) -> DecResult<(usize, Representation), ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((buf_index, ReprPrefixBit::INDEXED, mid_bit, index)) => {
                DecResult::Decoded((buf_index, Representation::Indexed { mid_bit, index }))
            }
            DecResult::Decoded((buf_index, ReprPrefixBit::INDEXEDWITHPOSTINDEX, _, index)) => {
                DecResult::Decoded((buf_index, Representation::IndexedWithPostIndex { index }))
            }
            DecResult::Decoded((buf_index, ReprPrefixBit::LITERALWITHINDEXING, mid_bit, index)) => {
                let res = ReprValueString::new(
                    ReprPrefixBit::LITERALWITHINDEXING,
                    mid_bit,
                    Name::Index(index),
                )
                .decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            DecResult::Decoded((
                buf_index,
                ReprPrefixBit::LITERALWITHPOSTINDEXING,
                mid_bit,
                index,
            )) => {
                let res = ReprValueString::new(
                    ReprPrefixBit::LITERALWITHPOSTINDEXING,
                    mid_bit,
                    Name::Index(index),
                )
                .decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            DecResult::Decoded((
                buf_index,
                ReprPrefixBit::LITERALWITHLITERALNAME,
                mid_bit,
                namelen,
            )) => {
                let res =
                    ReprNameAndValue::new(ReprPrefixBit::LITERALWITHLITERALNAME, mid_bit, namelen)
                        .decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            DecResult::NeedMore(inner) => DecResult::NeedMore(ReprIndex::from_inner(inner).into()),
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3errorQpack::ConnectionError(
                ErrorCode::DecompressionFailed,
            )),
        }
    }
}

pub(crate) struct FSPTwoIntergers;

impl FSPTwoIntergers {
    fn decode(
        self,
        buf: &[u8],
    ) -> DecResult<(usize, RequireInsertCount, bool, DeltaBase), FSPInner> {
        let mut buf_index = 1;
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let buf_len = buf.len();
        let mask = PrefixMask::REQUIREINSERTCOUNT;
        let ric = match IntegerDecoder::first_byte(buf[buf_index - 1], mask.0) {
            Ok(ric) => ric,
            Err(mut int) => {
                let res: usize;

                loop {
                    // If `buf` has been completely decoded here, return the current state.
                    buf_index += 1;
                    if buf_index >= buf_len {
                        return DecResult::NeedMore(self.into());
                    }
                    // Updates trailing bytes until we get the index.
                    match int.next_byte(buf[buf_index - 1]) {
                        Ok(None) => {}
                        Ok(Some(index)) => {
                            res = index;
                            break;
                        }
                        Err(e) => return e.into(),
                    }
                }
                res
            }
        };
        buf_index += 1;
        if buf_index >= buf_len {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[buf_index - 1];
        let signal = (byte & 0x80) != 0;
        let mask = PrefixMask::DELTABASE;
        let delta_base = match IntegerDecoder::first_byte(byte, mask.0) {
            Ok(delta_base) => delta_base,
            Err(mut int) => {
                let res: usize;
                loop {
                    // If `buf` has been completely decoded here, return the current state.
                    buf_index += 1;
                    if buf_index >= buf_len {
                        return DecResult::NeedMore(self.into());
                    }

                    // Updates trailing bytes until we get the index.
                    match int.next_byte(buf[buf_index - 1]) {
                        Ok(None) => {}
                        Ok(Some(index)) => {
                            res = index;
                            break;
                        }
                        Err(e) => return e.into(),
                    }
                }
                res
            }
        };

        DecResult::Decoded((
            buf_index,
            RequireInsertCount(ric),
            signal,
            DeltaBase(delta_base),
        ))
    }
}

macro_rules! decode_first_byte {
    ($struct_name:ident, $prefix_type:ty, $inner_type:ty,$trailing_bytes:ty) => {
        pub(crate) struct $struct_name;

        impl $struct_name {
            fn decode(
                self,
                buf: &[u8],
            ) -> DecResult<(usize, $prefix_type, MidBit, usize), $inner_type> {
                // If `buf` has been completely decoded here, return the current state.
                let buf_index = 1;
                if buf.is_empty() {
                    return DecResult::NeedMore(self.into());
                }

                let prefix = <$prefix_type>::from_u8(buf[buf_index - 1]);
                let mid_bit = prefix.prefix_midbit_value(buf[buf_index - 1]);
                let mask = prefix.prefix_index_mask();

                match IntegerDecoder::first_byte(buf[buf_index - 1], mask.0) {
                    // Return the PrefixBit and index part value.
                    Ok(idx) => DecResult::Decoded((buf_index, prefix, mid_bit, idx)),
                    // Index part value is longer than index(i.e. use all 1 to represent), so it needs more bytes to decode.
                    Err(int) => {
                        let res =
                            <$trailing_bytes>::new(prefix, mid_bit, int).decode(&buf[buf_index..]);
                        if let DecResult::Decoded((cnt, prefix, mid_bit, int)) = res {
                            return DecResult::Decoded((cnt + buf_index, prefix, mid_bit, int));
                        } else {
                            return res;
                        }
                    }
                }
            }
        }
    };
}

decode_first_byte!(
    InstFirstByte,
    EncoderInstPrefixBit,
    InstIndexInner,
    InstTrailingBytes
);
decode_first_byte!(
    ReprFirstByte,
    ReprPrefixBit,
    ReprIndexInner,
    ReprTrailingBytes
);

macro_rules! trailing_bytes_decoder {
    ($struct_name:ident, $prefix_type:ty, $inner_type:ty) => {
        pub(crate) struct $struct_name {
            prefix: $prefix_type,
            mid_bit: MidBit,
            index: IntegerDecoder,
        }

        impl $struct_name {
            fn new(prefix: $prefix_type, mid_bit: MidBit, index: IntegerDecoder) -> Self {
                Self {
                    prefix,
                    mid_bit,
                    index,
                }
            }

            fn decode(
                mut self,
                buf: &[u8],
            ) -> DecResult<(usize, $prefix_type, MidBit, usize), $inner_type> {
                let mut buf_index = 1;
                let buf_len = buf.len();
                loop {
                    // If `buf` has been completely decoded here, return the current state.
                    if buf_index > buf_len {
                        return DecResult::NeedMore(self.into());
                    }

                    // Updates trailing bytes until we get the index.
                    match self.index.next_byte(buf[buf_index - 1]) {
                        Ok(None) => {}
                        Ok(Some(index)) => {
                            return DecResult::Decoded((
                                buf_index,
                                self.prefix,
                                self.mid_bit,
                                index,
                            ));
                        }
                        Err(e) => return e.into(),
                    }
                    buf_index += 1;
                }
            }
        }
    };
}

trailing_bytes_decoder!(InstTrailingBytes, EncoderInstPrefixBit, InstIndexInner);
trailing_bytes_decoder!(ReprTrailingBytes, ReprPrefixBit, ReprIndexInner);

pub(crate) struct LengthTrailingBytes {
    is_huffman: bool,
    length: IntegerDecoder,
}

impl LengthTrailingBytes {
    fn new(is_huffman: bool, length: IntegerDecoder) -> Self {
        Self { is_huffman, length }
    }

    fn decode(mut self, buf: &[u8]) -> DecResult<(usize, Vec<u8>), LiteralString> {
        let mut buf_index = 1;
        let buf_len = buf.len();
        loop {
            if buf_index >= buf_len {
                return DecResult::NeedMore(self.into());
            }

            match (self.length.next_byte(buf[buf_index - 1]), self.is_huffman) {
                (Ok(None), _) => {}
                (Err(e), _) => return e.into(),
                (Ok(Some(length)), true) => {
                    let res = HuffmanStringBytes::new(length).decode(&buf[buf_index..]);
                    if let DecResult::Decoded((cnt, vec)) = res {
                        return DecResult::Decoded((cnt + buf_index, vec));
                    } else {
                        return res;
                    }
                }
                (Ok(Some(length)), false) => {
                    let res = AsciiStringBytes::new(length).decode(&buf[buf_index..]);
                    if let DecResult::Decoded((cnt, vec)) = res {
                        return DecResult::Decoded((cnt + buf_index, vec));
                    } else {
                        return res;
                    }
                }
            }
            buf_index += 1;
        }
    }
}

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

    fn decode(mut self, buf: &[u8]) -> DecResult<(usize, Vec<u8>), LiteralString> {
        match (buf.len() + self.octets.len()).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.octets.len();
                self.octets.extend_from_slice(&buf[..pos]);
                DecResult::Decoded((pos, self.octets))
            }
            Ordering::Less => {
                self.octets.extend_from_slice(buf);
                // let (_, mut remain_buf) = buf.split_at_mut(buf.len());
                DecResult::NeedMore(self.into())
            }
        }
    }
}

macro_rules! name_and_value_decoder {
    ($struct_name:ident, $prefix_type:ty, $inner_type:ty, $output_type:ty, $state_type:ty,$value_string:ty) => {
        pub(crate) struct $struct_name {
            prefix: $prefix_type,
            mid_bit: MidBit,
            inner: $inner_type,
        }

        impl $struct_name {
            fn new(prefix: $prefix_type, mid_bit: MidBit, namelen: usize) -> Self {
                Self::from_inner(prefix, mid_bit, AsciiStringBytes::new(namelen).into())
            }

            fn from_inner(prefix: $prefix_type, mid_bit: MidBit, inner: $inner_type) -> Self {
                Self {
                    prefix,
                    mid_bit,
                    inner,
                }
            }

            fn decode(self, buf: &[u8]) -> DecResult<(usize, $output_type), $state_type> {
                match self.inner.decode(buf) {
                    DecResult::Decoded((buf_index, octets)) => {
                        let res =
                            <$value_string>::new(self.prefix, self.mid_bit, Name::Literal(octets))
                                .decode(&buf[buf_index..]);
                        return_res!(res, buf_index)
                    }
                    DecResult::NeedMore(inner) => DecResult::NeedMore(
                        Self::from_inner(self.prefix, self.mid_bit, inner).into(),
                    ),
                    DecResult::Error(e) => e.into(),
                }
            }
        }
    };
}

name_and_value_decoder!(
    InstNameAndValue,
    EncoderInstPrefixBit,
    LiteralString,
    EncoderInstruction,
    InstDecodeState,
    InstValueString
);
name_and_value_decoder!(
    ReprNameAndValue,
    ReprPrefixBit,
    LiteralString,
    Representation,
    ReprDecodeState,
    ReprValueString
);

pub(crate) struct LengthFirstByte;

impl LengthFirstByte {
    fn decode(self, buf: &[u8]) -> DecResult<(usize, Vec<u8>), LiteralString> {
        let buf_index = 1;
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }

        match (
            IntegerDecoder::first_byte(buf[buf_index - 1], 0x7f),
            (buf[buf_index - 1] & 0x80) == 0x80,
        ) {
            (Ok(len), true) => {
                let res = HuffmanStringBytes::new(len).decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            (Ok(len), false) => {
                let res = AsciiStringBytes::new(len).decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
            (Err(int), huffman) => {
                let res = LengthTrailingBytes::new(huffman, int).decode(&buf[buf_index..]);
                return_res!(res, buf_index)
            }
        }
    }
}

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

    fn decode(mut self, buf: &[u8]) -> DecResult<(usize, Vec<u8>), LiteralString> {
        match (buf.len() + self.read).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.read;
                if self.huffman.decode(&buf[..pos]).is_err() {
                    return H3errorQpack::ConnectionError(DecompressionFailed).into();
                }
                // let (_, mut remain_buf) = buf.split_at_mut(pos);
                match self.huffman.finish() {
                    Ok(vec) => DecResult::Decoded((pos, vec)),
                    Err(_) => H3errorQpack::ConnectionError(DecompressionFailed).into(),
                }
            }
            Ordering::Less => {
                if self.huffman.decode(buf).is_err() {
                    return H3errorQpack::ConnectionError(DecompressionFailed).into();
                }
                self.read += buf.len();
                // let (_, mut remain_buf) = buf.split_at_mut(buf.len());
                DecResult::NeedMore(self.into())
            }
        }
    }
}

#[derive(Clone)]
pub(crate) enum Name {
    Index(usize),
    Literal(Vec<u8>),
}

pub(crate) struct InstValueString {
    inst: EncoderInstPrefixBit,
    mid_bit: MidBit,
    name: Name,
    inner: LiteralString,
}

impl InstValueString {
    fn new(inst: EncoderInstPrefixBit, mid_bit: MidBit, name: Name) -> Self {
        Self::from_inner(inst, mid_bit, name, LengthFirstByte.into())
    }

    fn from_inner(
        inst: EncoderInstPrefixBit,
        mid_bit: MidBit,
        name: Name,
        inner: LiteralString,
    ) -> Self {
        Self {
            inst,
            mid_bit,
            name,
            inner,
        }
    }

    fn decode(self, buf: &[u8]) -> DecResult<(usize, EncoderInstruction), InstDecodeState> {
        match (self.inst, self.inner.decode(buf)) {
            (EncoderInstPrefixBit::INSERTWITHINDEX, DecResult::Decoded((buf_index, value))) => {
                DecResult::Decoded((
                    buf_index,
                    EncoderInstruction::InsertWithIndex {
                        mid_bit: self.mid_bit,
                        name: self.name,
                        value,
                    },
                ))
            }
            (EncoderInstPrefixBit::INSERTWITHLITERAL, DecResult::Decoded((buf_index, value))) => {
                DecResult::Decoded((
                    buf_index,
                    EncoderInstruction::InsertWithLiteral {
                        mid_bit: self.mid_bit,
                        name: self.name,
                        value,
                    },
                ))
            }
            (_, _) => Error(H3errorQpack::ConnectionError(DecompressionFailed)),
        }
    }
}

pub(crate) struct ReprValueString {
    repr: ReprPrefixBit,
    mid_bit: MidBit,
    name: Name,
    inner: LiteralString,
}

impl ReprValueString {
    fn new(repr: ReprPrefixBit, mid_bit: MidBit, name: Name) -> Self {
        Self::from_inner(repr, mid_bit, name, LengthFirstByte.into())
    }

    fn from_inner(repr: ReprPrefixBit, mid_bit: MidBit, name: Name, inner: LiteralString) -> Self {
        Self {
            repr,
            mid_bit,
            name,
            inner,
        }
    }

    fn decode(self, buf: &[u8]) -> DecResult<(usize, Representation), ReprDecodeState> {
        match (self.repr, self.inner.decode(buf)) {
            (ReprPrefixBit::LITERALWITHINDEXING, DecResult::Decoded((buf_index, value))) => {
                DecResult::Decoded((
                    buf_index,
                    Representation::LiteralWithIndexing {
                        mid_bit: self.mid_bit,
                        name: self.name,
                        value,
                    },
                ))
            }
            (ReprPrefixBit::LITERALWITHPOSTINDEXING, DecResult::Decoded((buf_index, value))) => {
                DecResult::Decoded((
                    buf_index,
                    Representation::LiteralWithPostIndexing {
                        mid_bit: self.mid_bit,
                        name: self.name,
                        value,
                    },
                ))
            }
            (ReprPrefixBit::LITERALWITHLITERALNAME, DecResult::Decoded((buf_index, value))) => {
                DecResult::Decoded((
                    buf_index,
                    Representation::LiteralWithLiteralName {
                        mid_bit: self.mid_bit,
                        name: self.name,
                        value,
                    },
                ))
            }
            (_, _) => Error(H3errorQpack::ConnectionError(DecompressionFailed)),
        }
    }
}

/// Decoder's possible returns during the decoding process.
pub(crate) enum DecResult<D, S> {
    /// Decoder has got a `D`. Users can continue to call `encode` to try to
    /// get the next `D`.
    Decoded(D),

    /// Decoder needs more bytes to decode to get a `D`. Returns the current
    /// decoding state `S`.
    NeedMore(S),

    /// Errors that may occur when decoding.
    Error(H3errorQpack),
}

impl<D, S> From<H3errorQpack> for DecResult<D, S> {
    fn from(e: H3errorQpack) -> Self {
        DecResult::Error(e)
    }
}
