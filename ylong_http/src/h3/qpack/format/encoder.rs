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

use crate::h3::parts::Parts;
use crate::h3::qpack::error::ErrorCode::DecoderStreamError;
use crate::h3::qpack::error::H3errorQpack;
use crate::h3::qpack::format::decoder::DecResult;
use crate::h3::qpack::integer::{Integer, IntegerDecoder, IntegerEncoder};
use crate::h3::qpack::table::{DynamicTable, Field, TableIndex, TableSearcher};
use crate::h3::qpack::{DecoderInstPrefixBit, DecoderInstruction, EncoderInstruction, PrefixMask};
use crate::headers::HeadersIntoIter;
use crate::h3::pseudo::PseudoHeaders;
use std::arch::asm;
use std::cmp::{max, Ordering};
use std::collections::{HashMap, VecDeque};
use std::result;
use std::sync::Arc;

pub struct ReprEncoder<'a> {
    table: &'a mut DynamicTable,
    draining_index: usize,
    allow_post: bool,
    insert_length: &'a mut usize,
}

impl<'a> ReprEncoder<'a> {
    /// Creates a new, empty `ReprEncoder`.
    /// # Examples
//     ```no_run
//     use ylong_http::h3::qpack::table::DynamicTable;
//     use ylong_http::h3::qpack::format::encoder::ReprEncoder;
//     let mut table = DynamicTable::new(4096);
//     let mut insert_length = 0;
//     let mut encoder = ReprEncoder::new(&mut table, 0, true, &mut insert_length);
//     ```
    pub fn new(
        table: &'a mut DynamicTable,
        draining_index: usize,
        allow_post: bool,
        insert_length: &'a mut usize,
    ) -> Self {
        Self {
            table,
            draining_index,
            allow_post,
            insert_length,
        }
    }

    /// written to `buffer` and the length of the decoded content will be returned.
    /// # Examples
//     ```no_run
//     use std::collections::VecDeque;use ylong_http::h3::qpack::table::DynamicTable;
//     use ylong_http::h3::qpack::format::encoder::ReprEncoder;
//     let mut table = DynamicTable::new(4096);
//     let mut insert_length = 0;
//     let mut encoder = ReprEncoder::new(&mut table, 0, true, &mut insert_length);
//     let mut qpack_buffer = [0u8; 1024];
//     let mut stream_buffer = [0u8; 1024]; // stream buffer
//     let mut insert_list = VecDeque::new(); // fileds to insert
//     let mut required_insert_count = 0; // RFC required.
//     let mut field_iter = None; // for field iterator
//     let mut field_state = None; // for field encode state
//     encoder.encode(&mut field_iter, &mut field_state, &mut qpack_buffer, &mut stream_buffer, &mut insert_list, &mut required_insert_count);
    pub(crate) fn encode(
        &mut self,
        field_iter: &mut Option<PartsIter>,
        field_state: &mut Option<ReprEncodeState>,
        encoder_buffer: &mut [u8],
        stream_buffer: &mut [u8],
        insert_list: &mut VecDeque<(Field, String)>,
        required_insert_count: &mut usize,
    ) -> (usize, usize) {
        let mut cur_encoder = 0;
        let mut cur_stream = 0;
        let mut base = self.table.insert_count;
        if let Some(mut iter) = field_iter.take() {
            while let Some((h, v)) = iter.next() {
                let searcher = TableSearcher::new(self.table);
                let mut stream_result: Result<usize, ReprEncodeState> = Result::Ok(0);
                let mut encoder_result: Result<usize, ReprEncodeState> = Result::Ok(0);
                let static_index = searcher.find_index_static(&h, &v);
                if static_index != Some(TableIndex::None) {
                    if let Some(TableIndex::Field(index)) = static_index {
                        // Encode as index in static table
                        stream_result =
                            Indexed::new(index, true).encode(&mut stream_buffer[cur_stream..]);
                    }
                } else {
                    let mut dynamic_index = searcher.find_index_dynamic(&h, &v);
                    let static_name_index = searcher.find_index_name_static(&h, &v);
                    let mut dynamic_name_index = Some(TableIndex::None);
                    if dynamic_index == Some(TableIndex::None) || !self.should_index(&dynamic_index)
                    {
                        // if index is close to eviction, drop it and use duplicate
                        // let dyn_index = dynamic_index.clone();
                        // dynamic_index = Some(TableIndex::None);
                        let mut is_duplicate = false;
                        if static_name_index == Some(TableIndex::None) {
                            dynamic_name_index = searcher.find_index_name_dynamic(&h, &v);
                        }

                        if self.table.have_enough_space(&h, &v, self.insert_length) {
                            if !self.should_index(&dynamic_index) {
                                if let Some(TableIndex::Field(index)) = dynamic_index {
                                    encoder_result = Duplicate::new(base - index - 1)
                                        .encode(&mut encoder_buffer[cur_encoder..]);
                                    self.table.update(h.clone(), v.clone());
                                    base = max(base, self.table.insert_count);
                                    dynamic_index =
                                        Some(TableIndex::Field(self.table.insert_count - 1));
                                    is_duplicate = true;
                                }
                            } else {
                                encoder_result = match (
                                    &static_name_index,
                                    &dynamic_name_index,
                                    self.should_index(&dynamic_name_index),
                                ) {
                                    // insert with name reference in static table
                                    (Some(TableIndex::FieldName(index)), _, _) => {
                                        InsertWithName::new(
                                            *index,
                                            v.clone().into_bytes(),
                                            false,
                                            true,
                                        )
                                        .encode(&mut encoder_buffer[cur_encoder..])
                                    }
                                    // insert with name reference in dynamic table
                                    (_, Some(TableIndex::FieldName(index)), true) => {
                                        // convert abs index to rel index
                                        InsertWithName::new(
                                            base - index - 1,
                                            v.clone().into_bytes(),
                                            false,
                                            false,
                                        )
                                        .encode(&mut encoder_buffer[cur_encoder..])
                                    }
                                    // Duplicate
                                    (_, Some(TableIndex::FieldName(index)), false) => {
                                        let res = Duplicate::new(*index)
                                            .encode(&mut encoder_buffer[cur_encoder..]);
                                        self.table.update(h.clone(), v.clone());
                                        base = max(base, self.table.insert_count);
                                        dynamic_name_index = Some(TableIndex::FieldName(
                                            self.table.insert_count - 1,
                                        ));
                                        is_duplicate = true;
                                        res
                                    }
                                    // insert with literal name
                                    (_, _, _) => InsertWithLiteral::new(
                                        h.clone().into_string().into_bytes(),
                                        v.clone().into_bytes(),
                                        false,
                                    )
                                    .encode(&mut encoder_buffer[cur_encoder..]),
                                }
                            };
                            if self.table.size() + h.len() + v.len() + 32 >= self.table.capacity() {
                                self.draining_index += 1;
                            }
                            insert_list.push_back((h.clone(), v.clone()));
                            *self.insert_length += h.len() + v.len() + 32;
                        }
                        if self.allow_post && !is_duplicate {
                            for (post_index, (t_h, t_v)) in insert_list.iter().enumerate() {
                                if t_h == &h && t_v == &v {
                                    dynamic_index = Some(TableIndex::Field(post_index))
                                }
                                if t_h == &h {
                                    dynamic_name_index = Some(TableIndex::FieldName(post_index));
                                }
                            }
                        }
                    }

                    if dynamic_index == Some(TableIndex::None) {
                        if dynamic_name_index != Some(TableIndex::None) {
                            //Encode with name reference in dynamic table
                            if let Some(TableIndex::FieldName(index)) = dynamic_name_index {
                                // use post-base index
                                if base <= index {
                                    stream_result = IndexingWithPostName::new(
                                        index - base,
                                        v.clone().into_bytes(),
                                        false,
                                        false,
                                    )
                                    .encode(&mut stream_buffer[cur_stream..]);
                                } else {
                                    stream_result = IndexingWithName::new(
                                        base - index - 1,
                                        v.clone().into_bytes(),
                                        false,
                                        false,
                                        false,
                                    )
                                    .encode(&mut stream_buffer[cur_stream..]);
                                }
                                *required_insert_count = max(*required_insert_count, index + 1);
                            }
                        } else {
                            // Encode with name reference in static table
                            // or Encode as Literal
                            if static_name_index != Some(TableIndex::None) {
                                if let Some(TableIndex::FieldName(index)) = static_name_index {
                                    stream_result = IndexingWithName::new(
                                        index,
                                        v.into_bytes(),
                                        false,
                                        true,
                                        false,
                                    )
                                    .encode(&mut stream_buffer[cur_stream..]);
                                }
                            } else {
                                stream_result = IndexingWithLiteral::new(
                                    h.into_string().into_bytes(),
                                    v.into_bytes(),
                                    false,
                                    false,
                                )
                                .encode(&mut stream_buffer[cur_stream..]);
                            }
                        }
                    } else {
                        assert!(dynamic_index != Some(TableIndex::None));
                        // Encode with index in dynamic table
                        if let Some(TableIndex::Field(index)) = dynamic_index {
                            // use post-base index
                            if base <= index {
                                stream_result = IndexedWithPostName::new(index - base)
                                    .encode(&mut stream_buffer[cur_stream..]);
                            } else {
                                stream_result = Indexed::new(base - index - 1, false)
                                    .encode(&mut stream_buffer[cur_stream..]);
                            }
                            *required_insert_count = max(*required_insert_count, index + 1);
                        }
                    }
                }

                match (encoder_result, stream_result) {
                    (Ok(encoder_size), Ok(stream_size)) => {
                        cur_stream += stream_size;
                        cur_encoder += encoder_size;
                    }
                    (Err(state), Ok(_)) => {
                        *field_iter = Some(iter);
                        *field_state = Some(state);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                    (Ok(_), Err(state)) => {
                        *field_iter = Some(iter);
                        *field_state = Some(state);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                    (Err(_), Err(state)) => {
                        *field_iter = Some(iter);
                        *field_state = Some(state);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                }
            }
        }

        (cur_encoder, cur_stream)
    }
    // ## 2.1.1.1. Avoiding Prohibited Insertions
    // To ensure that the encoder is not prevented from adding new entries, the encoder can
    // avoid referencing entries that are close to eviction. Rather than reference such an
    // entry, the encoder can emit a Duplicate instruction (Section 4.3.4) and reference
    // the duplicate instead.
    //
    // Determining which entries are too close to eviction to reference is an encoder preference.
    // One heuristic is to target a fixed amount of available space in the dynamic table:
    // either unused space or space that can be reclaimed by evicting non-blocking entries.
    // To achieve this, the encoder can maintain a draining index, which is the smallest
    // absolute index (Section 3.2.4) in the dynamic table that it will emit a reference for.
    // As new entries are inserted, the encoder increases the draining index to maintain the
    // section of the table that it will not reference. If the encoder does not create new
    // references to entries with an absolute index lower than the draining index, the number
    // of unacknowledged references to those entries will eventually become zero, allowing
    // them to be evicted.
    //
    //     <-- Newer Entries          Older Entries -->
    // (Larger Indices)       (Smaller Indices)
    // +--------+---------------------------------+----------+
    // | Unused |          Referenceable          | Draining |
    // | Space  |             Entries             | Entries  |
    // +--------+---------------------------------+----------+
    // ^                                 ^          ^
    // |                                 |          |
    // Insertion Point                 Draining Index  Dropping
    // Point
    pub(crate) fn should_index(&self, index: &Option<TableIndex>) -> bool {
        match index {
            Some(TableIndex::Field(x)) => {
                if *x < self.draining_index {
                    return false;
                }
                true
            }
            Some(TableIndex::FieldName(x)) => {
                if *x < self.draining_index {
                    return false;
                }
                true
            }
            _ => true,
        }
    }
}

pub(crate) enum ReprEncodeState {
    SetCap(SetCap),
    Indexed(Indexed),
    InsertWithName(InsertWithName),
    InsertWithLiteral(InsertWithLiteral),
    IndexingWithName(IndexingWithName),
    IndexingWithPostName(IndexingWithPostName),
    IndexingWithLiteral(IndexingWithLiteral),
    IndexedWithPostName(IndexedWithPostName),
    Duplicate(Duplicate),
}

pub(crate) struct SetCap {
    capacity: Integer,
}

impl SetCap {
    fn from(capacity: Integer) -> Self {
        Self { capacity }
    }

    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: Integer::index(0x20, capacity, PrefixMask::SETCAP.0),
        }
    }

    pub(crate) fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.capacity
            .encode(dst)
            .map_err(|e| ReprEncodeState::SetCap(SetCap::from(e)))
    }
}

pub(crate) struct Duplicate {
    index: Integer,
}

impl Duplicate {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self {
            index: Integer::index(0x00, index, PrefixMask::DUPLICATE.0),
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::Duplicate(Duplicate::from(e)))
    }
}

pub(crate) struct Indexed {
    index: Integer,
}

impl Indexed {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize, is_static: bool) -> Self {
        if is_static {
            // in static table
            Self {
                index: Integer::index(0xc0, index, PrefixMask::INDEXED.0),
            }
        } else {
            // in dynamic table
            Self {
                index: Integer::index(0x80, index, PrefixMask::INDEXED.0),
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::Indexed(Indexed::from(e)))
    }
}

pub(crate) struct IndexedWithPostName {
    index: Integer,
}

impl IndexedWithPostName {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self {
            index: Integer::index(0x10, index, PrefixMask::INDEXINGWITHPOSTNAME.0),
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexedWithPostName(IndexedWithPostName::from(e)))
    }
}

pub(crate) struct InsertWithName {
    inner: IndexAndValue,
}

impl InsertWithName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(index: usize, value: Vec<u8>, is_huffman: bool, is_static: bool) -> Self {
        if is_static {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0xc0, index, PrefixMask::INSERTWITHINDEX.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x80, index, PrefixMask::INSERTWITHINDEX.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithName(InsertWithName::from(e)))
    }
}

pub(crate) struct IndexingWithName {
    inner: IndexAndValue,
}

impl IndexingWithName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(
        index: usize,
        value: Vec<u8>,
        is_huffman: bool,
        is_static: bool,
        no_permit: bool,
    ) -> Self {
        match (no_permit, is_static) {
            (true, true) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x70, index, PrefixMask::INDEXINGWITHNAME.0)
                    .set_value(value, is_huffman),
            },
            (true, false) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x60, index, PrefixMask::INDEXINGWITHNAME.0)
                    .set_value(value, is_huffman),
            },
            (false, true) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x50, index, PrefixMask::INDEXINGWITHNAME.0)
                    .set_value(value, is_huffman),
            },
            (false, false) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x40, index, PrefixMask::INDEXINGWITHNAME.0)
                    .set_value(value, is_huffman),
            },
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexingWithName(IndexingWithName::from(e)))
    }
}

pub(crate) struct IndexingWithPostName {
    inner: IndexAndValue,
}

impl IndexingWithPostName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(index: usize, value: Vec<u8>, is_huffman: bool, no_permit: bool) -> Self {
        if no_permit {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x08, index, PrefixMask::INDEXINGWITHPOSTNAME.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x00, index, PrefixMask::INDEXINGWITHPOSTNAME.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexingWithPostName(IndexingWithPostName::from(e)))
    }
}

pub(crate) struct IndexingWithLiteral {
    inner: NameAndValue,
}

impl IndexingWithLiteral {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool, no_permit: bool) -> Self {
        match (no_permit, is_huffman) {
            (true, true) => Self {
                inner: NameAndValue::new()
                    .set_index(0x38, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (true, false) => Self {
                inner: NameAndValue::new()
                    .set_index(0x30, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (false, true) => Self {
                inner: NameAndValue::new()
                    .set_index(0x28, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (false, false) => Self {
                inner: NameAndValue::new()
                    .set_index(0x20, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithLiteral(InsertWithLiteral::from(e)))
    }
}

pub(crate) struct InsertWithLiteral {
    inner: NameAndValue,
}

impl InsertWithLiteral {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        if is_huffman {
            Self {
                inner: NameAndValue::new()
                    .set_index(0x60, name.len(), PrefixMask::INSERTWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        } else {
            Self {
                inner: NameAndValue::new()
                    .set_index(0x40, name.len(), PrefixMask::INSERTWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithLiteral(InsertWithLiteral::from(e)))
    }
}

pub(crate) struct IndexAndValue {
    index: Option<Integer>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}
macro_rules! check_and_encode {
    ($item: expr, $dst: expr, $cur: expr, $self: expr) => {{
        if let Some(i) = $item.take() {
            match i.encode($dst) {
                Ok(len) => $cur += len,
                Err(e) => {
                    $item = Some(e);
                    return Err($self);
                }
            };
        }
    }};
}
impl IndexAndValue {
    fn new() -> Self {
        Self {
            index: None,
            value_length: None,
            value_octets: None,
        }
    }

    fn set_index(mut self, pre: u8, index: usize, mask: u8) -> Self {
        self.index = Some(Integer::index(pre, index, mask));
        self
    }

    fn set_value(mut self, value: Vec<u8>, is_huffman: bool) -> Self {
        self.value_length = Some(Integer::length(value.len(), is_huffman));
        self.value_octets = Some(Octets::new(value));
        self
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;
        check_and_encode!(self.index, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_octets, &mut dst[cur..], cur, self);
        Ok(cur)
    }
}

pub(crate) struct NameAndValue {
    index: Option<Integer>,
    name_length: Option<Integer>,
    name_octets: Option<Octets>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}

impl NameAndValue {
    fn new() -> Self {
        Self {
            index: None,
            name_length: None,
            name_octets: None,
            value_length: None,
            value_octets: None,
        }
    }

    fn set_index(mut self, pre: u8, index: usize, mask: u8) -> Self {
        self.index = Some(Integer::index(pre, index, mask));
        self
    }

    fn set_name_and_value(mut self, name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        self.name_length = Some(Integer::length(name.len(), is_huffman));
        self.name_octets = Some(Octets::new(name));
        self.value_length = Some(Integer::length(value.len(), is_huffman));
        self.value_octets = Some(Octets::new(value));
        self
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;
        check_and_encode!(self.index, &mut dst[cur..], cur, self);
        // check_and_encode!(self.name_length, &mut dst[cur..], cur, self); //no need for qpack cause it in index.
        check_and_encode!(self.name_octets, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_octets, &mut dst[cur..], cur, self);
        Ok(cur)
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

state_def!(InstDecodeState, DecoderInstruction, DecInstIndex);
pub(crate) struct DecInstDecoder<'a> {
    buf: &'a [u8],
}

impl<'a> DecInstDecoder<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub(crate) fn decode(
        &mut self,
        ins_state: &mut Option<InstDecodeState>,
    ) -> Result<Option<DecoderInstruction>, H3errorQpack> {
        if self.buf.is_empty() {
            return Ok(None);
        }

        match ins_state
            .take()
            .unwrap_or_else(|| InstDecodeState::DecInstIndex(DecInstIndex::new()))
            .decode(&mut self.buf)
        {
            // If `buf` is not enough to continue decoding a complete
            // `Representation`, `Ok(None)` will be returned. Users need to call
            // `save` to save the current state to a `ReprDecStateHolder`.
            DecResult::NeedMore(state) => {
                *ins_state = Some(state);
                Ok(None)
            }
            DecResult::Decoded(repr) => Ok(Some(repr)),

            DecResult::Error(error) => Err(error),
        }
    }
}
state_def!(
    DecInstIndexInner,
    (DecoderInstPrefixBit, usize),
    InstFirstByte,
    InstTrailingBytes
);

pub(crate) struct DecInstIndex {
    inner: DecInstIndexInner,
}

impl DecInstIndex {
    fn new() -> Self {
        Self::from_inner(InstFirstByte.into())
    }
    fn from_inner(inner: DecInstIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<DecoderInstruction, InstDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((DecoderInstPrefixBit::ACK, index)) => {
                DecResult::Decoded(DecoderInstruction::Ack { stream_id: index })
            }
            DecResult::Decoded((DecoderInstPrefixBit::STREAMCANCEL, index)) => {
                DecResult::Decoded(DecoderInstruction::StreamCancel { stream_id: index })
            }
            DecResult::Decoded((DecoderInstPrefixBit::INSERTCOUNTINCREMENT, index)) => {
                DecResult::Decoded(DecoderInstruction::InsertCountIncrement { increment: index })
            }
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3errorQpack::ConnectionError(DecoderStreamError)),
        }
    }
}

pub(crate) struct InstFirstByte;

impl InstFirstByte {
    fn decode(
        self,
        buf: &mut &[u8],
    ) -> DecResult<(DecoderInstPrefixBit, usize), DecInstIndexInner> {
        // If `buf` has been completely decoded here, return the current state.
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let inst = DecoderInstPrefixBit::from_u8(byte);
        let mask = inst.prefix_index_mask();

        // Moves the pointer of `buf` backward.
        *buf = &buf[1..];
        match IntegerDecoder::first_byte(byte, mask.0) {
            // Return the ReprPrefixBit and index part value.
            Ok(idx) => DecResult::Decoded((inst, idx)),
            // Index part value is longer than index(i.e. use all 1 to represent), so it needs more bytes to decode.
            Err(int) => InstTrailingBytes::new(inst, int).decode(buf),
        }
    }
}

pub(crate) struct InstTrailingBytes {
    inst: DecoderInstPrefixBit,
    index: IntegerDecoder,
}

impl InstTrailingBytes {
    fn new(inst: DecoderInstPrefixBit, index: IntegerDecoder) -> Self {
        Self { inst, index }
    }
    fn decode(
        mut self,
        buf: &mut &[u8],
    ) -> DecResult<(DecoderInstPrefixBit, usize), DecInstIndexInner> {
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
                Ok(Some(index)) => return DecResult::Decoded((self.inst, index)),
                Err(e) => return e.into(),
            }
        }
    }
}

pub(crate) struct Octets {
    src: Vec<u8>,
    idx: usize,
}

impl Octets {
    fn new(src: Vec<u8>) -> Self {
        Self { src, idx: 0 }
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;

        let input_len = self.src.len() - self.idx;
        let output_len = dst.len();

        if input_len == 0 {
            return Ok(cur);
        }

        match output_len.cmp(&input_len) {
            Ordering::Greater | Ordering::Equal => {
                dst[..input_len].copy_from_slice(&self.src[self.idx..]);
                cur += input_len;
                Ok(cur)
            }
            Ordering::Less => {
                dst[..].copy_from_slice(&self.src[self.idx..self.idx + output_len]);
                self.idx += output_len;
                Err(self)
            }
        }
    }
}

pub(crate) struct PartsIter {
    pseudo: PseudoHeaders,
    map: HeadersIntoIter,
    next_type: PartsIterDirection,
}

/// `PartsIterDirection` is the `PartsIter`'s direction to get the next header.
enum PartsIterDirection {
    Authority,
    Method,
    Path,
    Scheme,
    Status,
    Other,
}

impl PartsIter {
    /// Creates a new `PartsIter` from the given `Parts`.
    pub(crate) fn new(parts: Parts) -> Self {
        Self {
            pseudo: parts.pseudo,
            map: parts.map.into_iter(),
            next_type: PartsIterDirection::Method,
        }
    }

    /// Gets headers in the order of `Method`, `Status`, `Scheme`, `Path`,
    /// `Authority` and `Other`.
    fn next(&mut self) -> Option<(Field, String)> {
        loop {
            match self.next_type {
                PartsIterDirection::Method => match self.pseudo.take_method() {
                    Some(value) => return Some((Field::Method, value)),
                    None => self.next_type = PartsIterDirection::Status,
                },
                PartsIterDirection::Status => match self.pseudo.take_status() {
                    Some(value) => return Some((Field::Status, value)),
                    None => self.next_type = PartsIterDirection::Scheme,
                },
                PartsIterDirection::Scheme => match self.pseudo.take_scheme() {
                    Some(value) => return Some((Field::Scheme, value)),
                    None => self.next_type = PartsIterDirection::Path,
                },
                PartsIterDirection::Path => match self.pseudo.take_path() {
                    Some(value) => return Some((Field::Path, value)),
                    None => self.next_type = PartsIterDirection::Authority,
                },
                PartsIterDirection::Authority => match self.pseudo.take_authority() {
                    Some(value) => return Some((Field::Authority, value)),
                    None => self.next_type = PartsIterDirection::Other,
                },
                PartsIterDirection::Other => {
                    return self
                        .map
                        .next()
                        .map(|(h, v)| (Field::Other(h.to_string()), v.to_string().unwrap()));
                }
            }
        }
    }
}
