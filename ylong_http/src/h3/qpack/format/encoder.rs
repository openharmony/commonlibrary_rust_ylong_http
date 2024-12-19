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

use std::arch::asm;
use std::cmp::{max, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::{mem, result};

use crate::h3::parts::Parts;
use crate::h3::qpack::encoder::{EncodeMessage, UnackFields};
use crate::h3::qpack::error::ErrorCode::DecoderStreamError;
use crate::h3::qpack::error::QpackError;
use crate::h3::qpack::format::decoder::{DecResult, LiteralString};
use crate::h3::qpack::integer::{Integer, IntegerDecoder, IntegerEncoder};
use crate::h3::qpack::table::{DynamicTable, NameField, SearchResult, TableIndex, TableSearcher};
use crate::h3::qpack::{DecoderInstPrefixBit, DecoderInstruction, EncoderInstruction, PrefixMask};
use crate::h3::PseudoHeaders;
use crate::headers::HeadersIntoIter;
use crate::huffman::huffman_encode;

pub struct ReprEncoder<'a> {
    stream_id: u64,
    base: u64,
    is_huffman: bool,
    table: &'a mut DynamicTable,
}

impl<'a> ReprEncoder<'a> {
    pub fn new(stream_id: u64, base: u64, is_huffman: bool, table: &'a mut DynamicTable) -> Self {
        Self {
            stream_id,
            base,
            is_huffman,
            table,
        }
    }

    fn get_prefix(&self, max_ref: usize, prefix_buf: &mut Vec<u8>) {
        let required_insert_count = max_ref + 1;

        let mut wire_ric = 0;
        if max_ref != 0 {
            wire_ric = required_insert_count % (2 * self.table.max_entries()) + 1;
            Integer::index(0x00, wire_ric, 0xff).encode(prefix_buf);
            let base = self.base as usize;
            if base >= required_insert_count {
                Integer::index(0x00, base - required_insert_count, 0x7f).encode(prefix_buf);
            } else {
                Integer::index(0x80, required_insert_count - base - 1, 0x7f).encode(prefix_buf);
            }
        } else {
            Integer::index(0x00, wire_ric, 0xff).encode(prefix_buf);
            Integer::index(0x00, 0, 0x7f).encode(prefix_buf);
        }
    }

    pub(crate) fn iterate_encode_fields(
        &mut self,
        field_iter: &mut Option<PartsIter>,
        track_map: &mut HashMap<u64, UnackFields>,
        blocked_cnt: &mut usize,
        allow_block: bool,
        fields: &mut Vec<u8>,
        inst: &mut Vec<u8>,
    ) {
        let mut max_dynamic = 0;

        let mut ref_fields = HashSet::<usize>::new();

        if let Some(mut iter) = field_iter.take() {
            while let Some((field_h, field_v)) = iter.next() {
                if let Some(dyn_ref) =
                    self.encode_field(&mut ref_fields, field_h, field_v, inst, fields, allow_block)
                {
                    max_dynamic = max(max_dynamic, dyn_ref);
                }
            }
        }

        let enc_field_lines = mem::take(fields);

        self.get_prefix(max_dynamic, fields);
        fields.extend_from_slice(enc_field_lines.as_slice());
        if (max_dynamic as u64) > self.table.known_recved_count() {
            *blocked_cnt += 1;
        }

        if !ref_fields.is_empty() {
            track_map
                .entry(self.stream_id)
                .or_default()
                .update(ref_fields);
        }
    }

    fn encode_field(
        &mut self,
        ref_fields: &mut HashSet<usize>,
        field_h: NameField,
        field_v: String,
        inst: &mut Vec<u8>,
        fields: &mut Vec<u8>,
        allow_block: bool,
    ) -> Option<usize> {
        let mut dynamic_index = None;
        match self.search_filed_from_table(&field_h, field_v.as_str(), allow_block) {
            SearchResult::StaticIndex(index) => {
                Indexed::new(index, true).encode(fields);
            }
            SearchResult::StaticNameIndex(index) => {
                IndexingWithName::new(index, field_v.into_bytes(), self.is_huffman, true, false)
                    .encode(fields);
            }
            SearchResult::DynamicIndex(index) => {
                self.indexed_dynamic_field(index, fields);
                if ref_fields.insert(index) {
                    self.table.track_field(index);
                }
                dynamic_index = Some(index);
            }
            SearchResult::DynamicNameIndex(index) => {
                self.indexed_name_ref_with_literal_field(index, field_v, self.is_huffman, fields);
                if ref_fields.insert(index) {
                    self.table.track_field(index)
                }
                dynamic_index = Some(index);
            }
            SearchResult::NotFound => {
                // allow_block允许插入或引用未确认的index
                if allow_block {
                    if let Some(index) = self.add_field_to_dynamic(
                        field_h.clone(),
                        field_v.clone(),
                        inst,
                        self.is_huffman,
                    ) {
                        self.indexed_dynamic_field(index, fields);
                        ref_fields.insert(index);
                        self.table.track_field(index);
                        dynamic_index = Some(index);
                    } else {
                        IndexingWithLiteral::new(
                            field_h.to_string().into_bytes(),
                            field_v.into_bytes(),
                            self.is_huffman,
                            false,
                        )
                        .encode(fields);
                    }
                } else {
                    IndexingWithLiteral::new(
                        field_h.to_string().into_bytes(),
                        field_v.into_bytes(),
                        self.is_huffman,
                        false,
                    )
                    .encode(fields);
                }
            }
        }
        dynamic_index
    }

    fn indexed_dynamic_field(&self, index: usize, fields: &mut Vec<u8>) {
        if index as u64 >= self.base {
            IndexedWithPost::new(index - (self.base as usize)).encode(fields);
        } else {
            Indexed::new((self.base as usize) - index - 1, false).encode(fields);
        }
    }

    fn indexed_name_ref_with_literal_field(
        &self,
        index: usize,
        field_v: String,
        is_huffman: bool,
        fields: &mut Vec<u8>,
    ) {
        if index as u64 >= self.base {
            IndexingWithPostName::new(
                index - (self.base as usize),
                field_v.into_bytes(),
                is_huffman,
                false,
            )
            .encode(fields);
        } else {
            InsertWithName::new(
                (self.base as usize) - index - 1,
                field_v.into_bytes(),
                is_huffman,
                false,
            )
            .encode(fields);
        }
    }

    fn add_field_to_dynamic(
        &mut self,
        field_h: NameField,
        field_v: String,
        inst: &mut Vec<u8>,
        is_huffman: bool,
    ) -> Option<usize> {
        let header_size = field_h.len() + field_v.len() + 32;
        if let Some(len) = self.table.can_evict(header_size) {
            InsertWithLiteral::new(
                field_h.to_string().into_bytes(),
                field_v.clone().into_bytes(),
                is_huffman,
            )
            .encode(inst);
            self.table.evict_drained(len);
            Some(self.table.update(field_h, field_v))
        } else {
            IndexingWithLiteral::new(
                field_h.to_string().into_bytes(),
                field_v.into_bytes(),
                is_huffman,
                false,
            )
            .encode(inst);
            None
        }
    }

    fn search_filed_from_table(
        &mut self,
        h: &NameField,
        v: &str,
        allow_block: bool,
    ) -> SearchResult {
        let searcher = TableSearcher::new(self.table);
        let mut search_result = SearchResult::NotFound;
        match searcher.search_in_static(h, v) {
            TableIndex::Field(index) => {
                return SearchResult::StaticIndex(index);
            }
            TableIndex::FieldName(index) => {
                search_result = SearchResult::StaticNameIndex(index);
            }
            TableIndex::None => {}
        }

        match searcher.search_in_dynamic(h, v, allow_block) {
            TableIndex::Field(index) => {
                return SearchResult::DynamicIndex(index);
            }
            TableIndex::FieldName(index) => {
                if search_result == SearchResult::NotFound {
                    search_result = SearchResult::DynamicNameIndex(index);
                }
            }
            TableIndex::None => {}
        }

        search_result
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
    IndexedWithPostName(IndexedWithPost),
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
            capacity: Integer::index(0x20, capacity, PrefixMask::SET_CAP.0),
        }
    }

    pub(crate) fn encode(self, dst: &mut Vec<u8>) {
        self.capacity.encode(dst)
    }
}

pub(crate) struct Duplicate {
    index: Integer,
}

#[allow(unused)]
impl Duplicate {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self {
            index: Integer::index(0x00, index, PrefixMask::DUPLICATE.0),
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.index.encode(dst)
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

    fn encode(self, dst: &mut Vec<u8>) {
        self.index.encode(dst)
    }
}

pub(crate) struct IndexedWithPost {
    index: Integer,
}

impl IndexedWithPost {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self {
            index: Integer::index(0x10, index, PrefixMask::INDEXED_WITH_POST_NAME.0),
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.index.encode(dst)
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
                    .set_index(0xc0, index, PrefixMask::INSERT_WITH_INDEX.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x80, index, PrefixMask::INSERT_WITH_INDEX.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.inner.encode(dst)
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
                    .set_index(0x70, index, PrefixMask::INDEXING_WITH_NAME.0)
                    .set_value(value, is_huffman),
            },
            (true, false) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x60, index, PrefixMask::INDEXING_WITH_NAME.0)
                    .set_value(value, is_huffman),
            },
            (false, true) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x50, index, PrefixMask::INDEXING_WITH_NAME.0)
                    .set_value(value, is_huffman),
            },
            (false, false) => Self {
                inner: IndexAndValue::new()
                    .set_index(0x40, index, PrefixMask::INDEXING_WITH_NAME.0)
                    .set_value(value, is_huffman),
            },
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.inner.encode(dst)
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
                    .set_index(0x08, index, PrefixMask::INDEXED_WITH_POST_NAME.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x00, index, PrefixMask::INDEXED_WITH_POST_NAME.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.inner.encode(dst)
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
                    .set_index(0x38, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (true, false) => Self {
                inner: NameAndValue::new()
                    .set_index(0x30, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (false, true) => Self {
                inner: NameAndValue::new()
                    .set_index(0x28, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
            (false, false) => Self {
                inner: NameAndValue::new()
                    .set_index(0x20, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            },
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.inner.encode(dst)
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
                    .set_index(0x60, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        } else {
            Self {
                inner: NameAndValue::new()
                    .set_index(0x40, name.len(), PrefixMask::INSERT_WITH_LITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        self.inner.encode(dst)
    }
}

pub(crate) struct IndexAndValue {
    index: Option<Integer>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}
macro_rules! check_and_encode {
    ($item: expr, $dst: expr) => {{
        if let Some(i) = $item.take() {
            i.encode($dst)
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
        let huffman_value = Octets::new(value, is_huffman);
        self.value_length = Some(Integer::length(huffman_value.len(), is_huffman));
        self.value_octets = Some(huffman_value);
        self
    }

    fn encode(mut self, dst: &mut Vec<u8>) {
        check_and_encode!(self.index, dst);
        check_and_encode!(self.value_length, dst);
        check_and_encode!(self.value_octets, dst);
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
        let huffman_name = Octets::new(name, is_huffman);
        self.name_length = Some(Integer::length(huffman_name.len(), is_huffman));
        self.name_octets = Some(huffman_name);
        let huffman_value = Octets::new(value, is_huffman);
        self.value_length = Some(Integer::length(huffman_value.len(), is_huffman));
        self.value_octets = Some(huffman_value);
        self
    }

    fn encode(mut self, dst: &mut Vec<u8>) {
        check_and_encode!(self.index, dst);
        // check_and_encode!(self.name_length, &mut dst[cur..], cur, self); //no need
        // for qpack cause it in index.
        check_and_encode!(self.name_octets, dst);
        check_and_encode!(self.value_length, dst);
        check_and_encode!(self.value_octets, dst);
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
    ) -> Result<Option<DecoderInstruction>, QpackError> {
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
            DecResult::Decoded((DecoderInstPrefixBit::STREAM_CANCEL, index)) => {
                DecResult::Decoded(DecoderInstruction::StreamCancel { stream_id: index })
            }
            DecResult::Decoded((DecoderInstPrefixBit::INSERT_COUNT_INCREMENT, index)) => {
                DecResult::Decoded(DecoderInstruction::InsertCountIncrement { increment: index })
            }
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(QpackError::ConnectionError(DecoderStreamError)),
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
            // Index part value is longer than index(i.e. use all 1 to represent), so it needs more
            // bytes to decode.
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
}

impl Octets {
    fn new(src: Vec<u8>, is_huffman: bool) -> Self {
        if is_huffman {
            let mut dst = Vec::with_capacity(src.len());
            huffman_encode(src.as_slice(), dst.as_mut());
            Self { src: dst }
        } else {
            Self { src }
        }
    }

    fn encode(self, dst: &mut Vec<u8>) {
        dst.extend_from_slice(self.src.as_slice());
    }

    fn len(&self) -> usize {
        self.src.len()
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
    fn next(&mut self) -> Option<(NameField, String)> {
        loop {
            match self.next_type {
                PartsIterDirection::Method => match self.pseudo.take_method() {
                    Some(value) => return Some((NameField::Method, value)),
                    None => self.next_type = PartsIterDirection::Status,
                },
                PartsIterDirection::Status => match self.pseudo.take_status() {
                    Some(value) => return Some((NameField::Status, value)),
                    None => self.next_type = PartsIterDirection::Scheme,
                },
                PartsIterDirection::Scheme => match self.pseudo.take_scheme() {
                    Some(value) => return Some((NameField::Scheme, value)),
                    None => self.next_type = PartsIterDirection::Path,
                },
                PartsIterDirection::Path => match self.pseudo.take_path() {
                    Some(value) => return Some((NameField::Path, value)),
                    None => self.next_type = PartsIterDirection::Authority,
                },
                PartsIterDirection::Authority => match self.pseudo.take_authority() {
                    Some(value) => return Some((NameField::Authority, value)),
                    None => self.next_type = PartsIterDirection::Other,
                },
                PartsIterDirection::Other => {
                    return self
                        .map
                        .next()
                        .map(|(h, v)| (NameField::Other(h.to_string()), v.to_string().unwrap()));
                }
            }
        }
    }
}
