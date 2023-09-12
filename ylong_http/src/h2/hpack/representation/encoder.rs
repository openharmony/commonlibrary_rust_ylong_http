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
use std::collections::hash_map::IntoIter;

use crate::h2::hpack::integer::IntegerEncoder;
use crate::h2::hpack::representation::PrefixIndexMask;
use crate::h2::hpack::table::{DynamicTable, Header, TableIndex, TableSearcher};
use crate::h2::{Parts, PseudoHeaders};
use crate::headers::HeadersIntoIter;

/// Encoder implementation for decoding representation. The encode interface
/// supports segmented writing.
pub(crate) struct ReprEncoder<'a> {
    table: &'a mut DynamicTable,
    iter: Option<PartsIter>,
    state: Option<ReprEncodeState>,
}

impl<'a> ReprEncoder<'a> {
    /// Creates a new, empty `ReprEncoder`.
    pub(crate) fn new(table: &'a mut DynamicTable) -> Self {
        Self {
            table,
            iter: None,
            state: None,
        }
    }

    /// Loads states from a holder.
    pub(crate) fn load(&mut self, holder: &mut ReprEncStateHolder) {
        self.iter = holder.iter.take();
        self.state = holder.state.take();
    }

    /// Saves current state to a holder.
    pub(crate) fn save(self, holder: &mut ReprEncStateHolder) {
        holder.iter = self.iter;
        holder.state = self.state;
    }

    /// Decodes the contents of `self.iter` and `self.state`. The result will be
    /// written to `dst` and the length of the decoded content will be returned.
    ///
    /// If the `dst` is full, users can call this method again after preparing a
    /// new piece of memory, and the subsequent decoding part will be written
    /// into the new memory.
    ///
    /// # Note
    /// Decoding is complete only when `self.iter` and `self.state` are both
    /// `None`. It is recommended that users save the result to a
    /// `RecEncStateHolder` immediately after using the method.
    pub(crate) fn encode(&mut self, dst: &mut [u8]) -> usize {
        // If `dst` is empty, leave the state unchanged.
        if dst.is_empty() {
            return 0;
        }

        // `cur` is used to record the current write position of `dst`.
        let mut cur = 0;

        // If `self.state` is not `None`, we need to prioritize it.
        if let Some(state) = self.state.take() {
            match state.encode(&mut dst[cur..]) {
                Ok(size) => cur += size,
                Err(state) => {
                    // If `dst` is not long enough, save the current state.
                    self.state = Some(state);
                    return dst.len();
                }
            }
        }

        // If `self.iter` is not `None`, select the next one from the iterator
        // to decode.
        if let Some(mut iter) = self.iter.take() {
            while let Some((h, v)) = iter.next() {
                let searcher = TableSearcher::new(self.table);
                let result = match searcher.index(&h, &v) {
                    Some(TableIndex::Header(index)) => Indexed::new(index).encode(&mut dst[cur..]),
                    Some(TableIndex::HeaderName(index)) => {
                        // Update it to the dynamic table first, then decode it.
                        self.table.update(h.clone(), v.clone());
                        Indexing::new(index, v.into_bytes(), false).encode(&mut dst[cur..])
                    }
                    None => {
                        // Update it to the dynamic table first, then decode it.
                        self.table.update(h.clone(), v.clone());
                        IndexingWithName::new(h.into_string().into_bytes(), v.into_bytes(), false)
                            .encode(&mut dst[cur..])
                    }
                };
                match result {
                    Ok(size) => cur += size,
                    Err(state) => {
                        // If `dst` is not long enough, save the current state.
                        self.state = Some(state);
                        self.iter = Some(iter);
                        return dst.len();
                    }
                }
            }
        }
        cur
    }
}

/// `ReprEncStateHolder` is used to save the intermediate results of the
/// `ReprEncoder`.
pub(crate) struct ReprEncStateHolder {
    iter: Option<PartsIter>,
    state: Option<ReprEncodeState>,
}

impl ReprEncStateHolder {
    /// Creates a new, empty `ReprEncStateHolder`.
    pub(crate) fn new() -> Self {
        Self {
            iter: None,
            state: None,
        }
    }

    /// Creates a state based on the `Parts` to be encoded.
    pub(crate) fn set_parts(&mut self, parts: Parts) {
        self.iter = Some(PartsIter::new(parts))
    }

    /// Determines whether `self.iter` and `self.state` are empty. if they are
    /// empty, it means encoding is finished.
    pub(crate) fn is_empty(&self) -> bool {
        self.iter.is_none() && self.state.is_none()
    }
}

/// `PartsIter` is an iterator for traversing the contents of `Parts`.
struct PartsIter {
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
    fn new(parts: Parts) -> Self {
        Self {
            pseudo: parts.pseudo,
            map: parts.map.into_iter(),
            next_type: PartsIterDirection::Method,
        }
    }

    /// Gets headers in the order of `Method`, `Status`, `Scheme`, `Path`,
    /// `Authority` and `Other`.
    fn next(&mut self) -> Option<(Header, String)> {
        loop {
            match self.next_type {
                PartsIterDirection::Method => match self.pseudo.take_method() {
                    Some(value) => return Some((Header::Method, value)),
                    None => self.next_type = PartsIterDirection::Status,
                },
                PartsIterDirection::Status => match self.pseudo.take_status() {
                    Some(value) => return Some((Header::Status, value)),
                    None => self.next_type = PartsIterDirection::Scheme,
                },
                PartsIterDirection::Scheme => match self.pseudo.take_scheme() {
                    Some(value) => return Some((Header::Scheme, value)),
                    None => self.next_type = PartsIterDirection::Path,
                },
                PartsIterDirection::Path => match self.pseudo.take_path() {
                    Some(value) => return Some((Header::Path, value)),
                    None => self.next_type = PartsIterDirection::Authority,
                },
                PartsIterDirection::Authority => match self.pseudo.take_authority() {
                    Some(value) => return Some((Header::Authority, value)),
                    None => self.next_type = PartsIterDirection::Other,
                },
                PartsIterDirection::Other => {
                    return self
                        .map
                        .next()
                        .map(|(h, v)| (Header::Other(h.to_string()), v.to_string().unwrap()))
                }
            }
        }
    }
}

/// Possible states in `Representation` process.
pub(crate) enum ReprEncodeState {
    Indexed(Indexed),
    Indexing(Indexing),
    IndexingWithName(IndexingWithName),
    WithoutIndexing(WithoutIndexing),
    WithoutIndexingWithName(WithoutIndexingWithName),
    NeverIndexed(NeverIndexed),
    NeverIndexedWithName(NeverIndexedWithName),
}

impl ReprEncodeState {
    pub(crate) fn encode(self, dst: &mut [u8]) -> Result<usize, Self> {
        match self {
            Self::Indexed(s) => s.encode(dst),
            Self::Indexing(s) => s.encode(dst),
            Self::IndexingWithName(s) => s.encode(dst),
            Self::WithoutIndexing(s) => s.encode(dst),
            Self::WithoutIndexingWithName(s) => s.encode(dst),
            Self::NeverIndexed(s) => s.encode(dst),
            Self::NeverIndexedWithName(s) => s.encode(dst),
        }
    }
}

pub(crate) struct Indexed {
    index: Integer,
}
impl Indexed {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self {
            index: Integer::index(index, PrefixIndexMask::INDEXED.0, 0x80),
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::Indexed(Indexed::from(e)))
    }
}

pub(crate) struct Indexing {
    inner: IndexAndValue,
}
impl Indexing {
    fn new(index: usize, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: IndexAndValue::new()
                .set_index(index, PrefixIndexMask::LITERAL_WITH_INDEXING.0, 0x40)
                .set_value(value, is_huffman),
        }
    }

    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::Indexing(Indexing::from(e)))
    }
}

pub(crate) struct IndexingWithName {
    inner: NameAndValue,
}
impl IndexingWithName {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: NameAndValue::new()
                .set_index(PrefixIndexMask::LITERAL_WITH_INDEXING.0, 0x40)
                .set_name_and_value(name, value, is_huffman),
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexingWithName(IndexingWithName::from(e)))
    }
}

pub(crate) struct WithoutIndexing {
    inner: IndexAndValue,
}
impl WithoutIndexing {
    fn new(index: usize, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: IndexAndValue::new()
                .set_index(index, PrefixIndexMask::LITERAL_WITHOUT_INDEXING.0, 0x00)
                .set_value(value, is_huffman),
        }
    }

    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::WithoutIndexing(WithoutIndexing::from(e)))
    }
}

pub(crate) struct WithoutIndexingWithName {
    inner: NameAndValue,
}
impl WithoutIndexingWithName {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: NameAndValue::new()
                .set_index(PrefixIndexMask::LITERAL_WITHOUT_INDEXING.0, 0x00)
                .set_name_and_value(name, value, is_huffman),
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::WithoutIndexingWithName(WithoutIndexingWithName::from(e)))
    }
}

pub(crate) struct NeverIndexed {
    inner: IndexAndValue,
}
impl NeverIndexed {
    fn new(index: usize, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: IndexAndValue::new()
                .set_index(index, PrefixIndexMask::LITERAL_NEVER_INDEXED.0, 0x10)
                .set_value(value, is_huffman),
        }
    }

    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::NeverIndexed(NeverIndexed::from(e)))
    }
}

pub(crate) struct NeverIndexedWithName {
    inner: NameAndValue,
}
impl NeverIndexedWithName {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        Self {
            inner: NameAndValue::new()
                .set_index(PrefixIndexMask::LITERAL_NEVER_INDEXED.0, 0x10)
                .set_name_and_value(name, value, is_huffman),
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::NeverIndexedWithName(NeverIndexedWithName::from(e)))
    }
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

pub(crate) struct IndexAndValue {
    index: Option<Integer>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}

impl IndexAndValue {
    fn new() -> Self {
        Self {
            index: None,
            value_length: None,
            value_octets: None,
        }
    }

    fn set_index(mut self, index: usize, mask: u8, pre: u8) -> Self {
        self.index = Some(Integer::index(index, mask, pre));
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

    fn set_index(mut self, mask: u8, pre: u8) -> Self {
        self.index = Some(Integer::index(0, mask, pre));
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
        check_and_encode!(self.name_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.name_octets, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_octets, &mut dst[cur..], cur, self);
        Ok(cur)
    }
}

pub(crate) struct Integer {
    int: IntegerEncoder,
}

impl Integer {
    fn index(index: usize, mask: u8, pre: u8) -> Self {
        Self {
            int: IntegerEncoder::new(index, mask, pre),
        }
    }

    fn length(length: usize, is_huffman: bool) -> Self {
        Self {
            int: IntegerEncoder::new(length, 0x7f, u8::from(is_huffman)),
        }
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
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

#[cfg(test)]
mod ut_repre_encoder {
    use super::*;

    /// UT test cases for `ReprEncodeState::encode`.
    ///
    /// # Brief
    /// 1. Creates a `ReprEncodeState` for each possible state.
    /// 2. Calls `ReprEncodeState::encode` for each state.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_repr_encode_state_encode() {
        let mut buffer = [0u8; 256];

        let repr = ReprEncodeState::Indexed(Indexed::new(42));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr = ReprEncodeState::Indexing(Indexing::new(42, vec![0x0a, 0x0b, 0x0c], false));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr = ReprEncodeState::IndexingWithName(IndexingWithName::new(
            vec![0x0a, 0x0b, 0x0c],
            vec![0x1a, 0x1b, 0x1c],
            false,
        ));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr = ReprEncodeState::WithoutIndexing(WithoutIndexing::new(
            42,
            vec![0x2a, 0x2b, 0x2c],
            false,
        ));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr = ReprEncodeState::WithoutIndexingWithName(WithoutIndexingWithName::new(
            vec![0x3a, 0x3b, 0x3c],
            vec![0x4a, 0x4b, 0x4c],
            false,
        ));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr =
            ReprEncodeState::NeverIndexed(NeverIndexed::new(42, vec![0x5a, 0x5b, 0x5c], false));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());

        let repr = ReprEncodeState::NeverIndexedWithName(NeverIndexedWithName::new(
            vec![0x6a, 0x6b, 0x6c],
            vec![0x7a, 0x7b, 0x7c],
            false,
        ));
        let result = repr.encode(&mut buffer);
        assert!(result.is_ok());
    }
}
