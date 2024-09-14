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

use std::collections::{HashMap, HashSet};
use std::mem::take;

use ylong_runtime::iter::parallel::ParSplit;

use crate::h3::parts::Parts;
use crate::h3::qpack::decoder::FieldDecodeState::{Blocked, Decoded};
use crate::h3::qpack::error::ErrorCode::{DecompressionFailed, EncoderStreamError};
use crate::h3::qpack::error::{ErrorCode, NotClassified, QpackError};
use crate::h3::qpack::format::decoder::{
    EncInstDecoder, InstDecodeState, Name, ReprDecodeState, ReprDecoder,
};
use crate::h3::qpack::integer::Integer;
use crate::h3::qpack::table::NameField::Path;
use crate::h3::qpack::table::{DynamicTable, NameField, TableSearcher};
use crate::h3::qpack::{
    DeltaBase, EncoderInstPrefixBit, EncoderInstruction, MidBit, ReprPrefixBit, Representation,
    RequireInsertCount,
};

pub(crate) enum FieldDecodeState {
    Blocked,
    Decoded,
}

pub(crate) struct FiledLines {
    parts: Parts,
    header_size: usize,
}

pub(crate) struct ReprMessage {
    require_insert_count: usize,
    base: usize,
    // dynamic table
    repr_state: Option<ReprDecodeState>,
    remaining: Option<Vec<u8>>,
    // instruction decode state
    lines: FiledLines,
}

impl ReprMessage {
    pub(crate) fn new() -> Self {
        Self {
            require_insert_count: 0,
            base: 0,
            repr_state: None,
            remaining: None,
            lines: FiledLines {
                parts: Parts::new(),
                header_size: 0,
            },
        }
    }
}

pub struct QpackDecoder {
    // max header list size
    table: DynamicTable,
    // field decode state
    inst_state: Option<InstDecodeState>,
    streams: HashMap<u64, ReprMessage>,
    blocked: HashMap<u64, usize>,
    max_blocked_streams: usize,
    max_table_capacity: usize,
    max_field_section_size: usize,
}

impl QpackDecoder {
    pub(crate) fn new(max_blocked_streams: usize, max_table_capacity: usize) -> Self {
        Self {
            table: DynamicTable::with_empty(),
            inst_state: None,
            streams: HashMap::new(),
            blocked: HashMap::new(),
            max_blocked_streams,
            max_table_capacity,
            max_field_section_size: (1 << 62) - 1,
        }
    }

    pub(crate) fn finish_stream(&mut self, id: u64) -> Result<(), QpackError> {
        if self.blocked.contains_key(&id) {
            Err(QpackError::ConnectionError(ErrorCode::DecoderStreamError))
        } else {
            self.streams.remove(&id);
            Ok(())
        }
    }

    pub(crate) fn set_max_field_section_size(&mut self, size: usize) {
        self.max_field_section_size = size;
    }

    pub(crate) fn decode_ins(&mut self, buf: &[u8]) -> Result<Vec<u64>, QpackError> {
        let mut decoder = EncInstDecoder::new();
        let mut updater = Updater::new(&mut self.table);
        let mut cnt = 0;
        while cnt < buf.len() {
            match decoder.decode(&buf[cnt..], &mut self.inst_state)? {
                Some(inst) => match inst {
                    (offset, EncoderInstruction::SetCap { capacity }) => {
                        cnt += offset;
                        if capacity > self.max_table_capacity {
                            return Err(QpackError::ConnectionError(DecompressionFailed));
                        }
                        updater.update_capacity(capacity)?;
                    }
                    (
                        offset,
                        EncoderInstruction::InsertWithIndex {
                            mid_bit,
                            name,
                            value,
                        },
                    ) => {
                        cnt += offset;
                        updater.update_table(mid_bit, name, value)?;
                    }
                    (
                        offset,
                        EncoderInstruction::InsertWithLiteral {
                            mid_bit,
                            name,
                            value,
                        },
                    ) => {
                        cnt += offset;
                        updater.update_table(mid_bit, name, value)?;
                    }
                    (offset, EncoderInstruction::Duplicate { index }) => {
                        cnt += offset;
                        updater.duplicate(index)?;
                    }
                },
                None => break,
            }
        }

        let insert_count = self.table.insert_count();

        let unblocked = self
            .blocked
            .iter()
            .filter_map(|(id, required)| {
                if *required <= insert_count {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        self.blocked.retain(|_, required| *required > insert_count);

        Ok(unblocked)
    }

    /// User call `decoder_repr` once for decoding a complete field section,
    /// which start with the `field section prefix`:  0   1   2   3   4   5
    /// 6   7 +---+---+---+---+---+---+---+---+
    /// |   Required Insert Count (8+)  |
    /// +---+---------------------------+
    /// | S |      Delta Base (7+)      |
    /// +---+---------------------------+
    /// |      Encoded Field Lines    ...
    /// +-------------------------------+

    pub(crate) fn decode_repr(
        &mut self,
        buf: &[u8],
        stream_id: u64,
    ) -> Result<FieldDecodeState, QpackError> {
        if self.blocked.contains_key(&stream_id) {
            return Err(QpackError::InternalError(NotClassified::StreamBlocked));
        }
        let mut message = match self.streams.remove(&stream_id) {
            None => ReprMessage::new(),
            Some(mut message) => {
                if let Some(vec) = message.remaining.take() {
                    // A block cannot occur here because this is the stream expelled from the block.
                    self.decode_buffered_repr(vec.as_slice(), &mut message, stream_id)?;
                }
                message
            }
        };

        self.decode_buffered_repr(buf, &mut message, stream_id)
            .map(|state| {
                self.streams.insert(stream_id, message);
                state
            })
    }

    fn decode_buffered_repr(
        &mut self,
        buf: &[u8],
        message: &mut ReprMessage,
        stream_id: u64,
    ) -> Result<FieldDecodeState, QpackError> {
        if buf.is_empty() {
            return Ok(Decoded);
        }
        let mut decoder = ReprDecoder::new();
        let mut searcher =
            Searcher::new(self.max_field_section_size, &self.table, &mut message.lines);
        let mut cnt = 0;
        loop {
            match decoder.decode(&buf[cnt..], &mut message.repr_state)? {
                Some((offset, repr)) => match repr {
                    Representation::FieldSectionPrefix {
                        require_insert_count,
                        signal,
                        delta_base,
                    } => {
                        cnt += offset;
                        if require_insert_count.0 == 0 {
                            message.require_insert_count = 0;
                        } else {
                            let max_entries = searcher.table.max_entries();
                            let full_range = 2 * max_entries;
                            if require_insert_count.0 > full_range {
                                return Err(QpackError::ConnectionError(DecompressionFailed));
                            }
                            let max_value = searcher.table.insert_count() + max_entries;
                            let max_wrapped = (max_value / full_range) * full_range;
                            message.require_insert_count = max_wrapped + require_insert_count.0 - 1;

                            if message.require_insert_count > max_value {
                                if message.require_insert_count <= full_range {
                                    return Err(QpackError::ConnectionError(DecompressionFailed));
                                }
                                message.require_insert_count -= full_range;
                            }
                            if message.require_insert_count == 0 {
                                return Err(QpackError::ConnectionError(DecompressionFailed));
                            }
                        }
                        if signal {
                            message.base = message.require_insert_count - delta_base.0 - 1;
                        } else {
                            message.base = message.require_insert_count + delta_base.0;
                        }
                        searcher.base = message.base;
                        if message.require_insert_count > searcher.table.insert_count() {
                            if self.blocked.len() > self.max_blocked_streams {
                                return Err(QpackError::ConnectionError(DecompressionFailed));
                            }
                            self.blocked.insert(stream_id, message.require_insert_count);
                            message.remaining = Some(Vec::from(&buf[cnt..]));
                            return Ok(Blocked);
                        }
                    }
                    Representation::Indexed { mid_bit, index } => {
                        cnt += offset;
                        searcher.search(Representation::Indexed { mid_bit, index })?;
                    }
                    Representation::IndexedWithPostIndex { index } => {
                        cnt += offset;
                        searcher.search(Representation::IndexedWithPostIndex { index })?;
                    }
                    Representation::LiteralWithIndexing {
                        mid_bit,
                        name,
                        value,
                    } => {
                        cnt += offset;
                        searcher.search_literal_with_indexing(mid_bit, name, value)?;
                    }

                    Representation::LiteralWithPostIndexing {
                        mid_bit,
                        name,
                        value,
                    } => {
                        cnt += offset;
                        searcher.search_literal_with_post_indexing(mid_bit, name, value)?;
                    }
                    Representation::LiteralWithLiteralName {
                        mid_bit,
                        name,
                        value,
                    } => {
                        cnt += offset;
                        searcher.search_listeral_with_literal(mid_bit, name, value)?;
                    }
                },
                None => {
                    return Ok(Decoded);
                }
            }
        }
    }

    /// Users call `finish` to stop decoding a field section. And send an
    /// `Section Acknowledgment` to encoder: After processing an encoded
    /// field section whose declared Required Insert Count is not zero,
    /// the decoder emits a Section Acknowledgment instruction. The instruction
    /// starts with the '1' 1-bit pattern, followed by the field section's
    /// associated stream ID encoded as a 7-bit prefix integer
    ///  0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 1 |      Stream ID (7+)       |
    /// +---+---------------------------+
    /// # Examples(not run)
    pub fn finish(
        &mut self,
        stream_id: u64,
        buf: &mut Vec<u8>,
    ) -> Result<(Parts, Option<usize>), QpackError> {
        match self.streams.remove(&stream_id) {
            None => Err(QpackError::ConnectionError(DecompressionFailed)),
            Some(mut message) => {
                if message.repr_state.is_some() {
                    return Err(QpackError::ConnectionError(DecompressionFailed));
                }
                message.lines.header_size = 0;
                if message.require_insert_count > 0 {
                    let ack = Integer::index(0x80, stream_id as usize, 0x7f);

                    let mut res = Vec::new();
                    ack.encode(&mut res);
                    buf.extend_from_slice(res.as_slice());
                    return Ok((take(&mut message.lines.parts), Some(res.len())));
                }
                Ok((take(&mut message.lines.parts), None))
            }
        }
    }

    /// Users call `stream_cancel` to stop cancel a stream. And send an `Stream
    /// Cancellation` to encoder: When a stream is reset or reading is
    /// abandoned, the decoder emits a Stream Cancellation instruction. The
    /// instruction starts with the '01' 2-bit pattern, followed by the
    /// stream ID of the affected stream encoded as a 6-bit prefix integer.
    ///  0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 1 |     Stream ID (6+)    |
    /// +---+---+-----------------------+
    pub fn stream_cancel(&mut self, stream_id: u64, buf: &mut [u8]) -> Result<usize, QpackError> {
        if self.table.capacity() > 0 {
            self.blocked.remove(&stream_id);
            self.streams.remove(&stream_id);
            let ack = Integer::index(0x40, stream_id as usize, 0x3f);
            let mut res = Vec::new();
            ack.encode(&mut res);
            if res.len() > buf.len() {
                Err(QpackError::ConnectionError(DecompressionFailed))
            } else {
                buf[..res.len()].copy_from_slice(res.as_slice());
                Ok(res.len())
            }
        } else {
            Ok(0)
        }
    }
}

struct Updater<'a> {
    table: &'a mut DynamicTable,
}

impl<'a> Updater<'a> {
    fn new(table: &'a mut DynamicTable) -> Self {
        Self { table }
    }

    fn update_capacity(&mut self, capacity: usize) -> Result<(), QpackError> {
        self.table.update_size(capacity);
        Ok(())
    }

    fn update_table(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), QpackError> {
        let (f, v) =
            self.get_field_by_name_and_value(mid_bit, name, value, self.table.insert_count())?;
        self.table.update(f, v);
        Ok(())
    }

    fn duplicate(&mut self, index: usize) -> Result<(), QpackError> {
        let table_searcher = TableSearcher::new(self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.table.insert_count() - index - 1)
            .ok_or(QpackError::ConnectionError(EncoderStreamError))?;
        self.table.update(f, v);
        Ok(())
    }

    fn get_field_by_name_and_value(
        &self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
        insert_count: usize,
    ) -> Result<(NameField, String), QpackError> {
        let h = match name {
            Name::Index(index) => {
                let searcher = TableSearcher::new(self.table);
                if let Some(true) = mid_bit.t {
                    searcher
                        .find_field_name_static(index)
                        .ok_or(QpackError::ConnectionError(EncoderStreamError))?
                } else {
                    searcher
                        .find_field_name_dynamic(insert_count - index - 1)
                        .ok_or(QpackError::ConnectionError(EncoderStreamError))?
                }
            }
            Name::Literal(octets) => NameField::Other(
                String::from_utf8(octets)
                    .map_err(|_| QpackError::ConnectionError(EncoderStreamError))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| QpackError::ConnectionError(EncoderStreamError))?;
        Ok((h, v))
    }
}

struct Searcher<'a> {
    max_field_section_size: usize,
    table: &'a DynamicTable,
    lines: &'a mut FiledLines,
    base: usize,
}

impl<'a> Searcher<'a> {
    fn new(
        max_field_section_size: usize,
        table: &'a DynamicTable,
        lines: &'a mut FiledLines,
    ) -> Self {
        Self {
            max_field_section_size,
            table,
            lines,
            base: 0,
        }
    }

    fn search(&mut self, repr: Representation) -> Result<(), QpackError> {
        match repr {
            Representation::Indexed { mid_bit, index } => self.search_indexed(mid_bit, index),
            Representation::IndexedWithPostIndex { index } => self.search_post_indexed(index),
            _ => Ok(()),
        }
    }

    fn search_indexed(&mut self, mid_bit: MidBit, index: usize) -> Result<(), QpackError> {
        let table_searcher = TableSearcher::new(self.table);
        if let Some(true) = mid_bit.t {
            let (f, v) = table_searcher
                .find_field_static(index)
                .ok_or(QpackError::ConnectionError(DecompressionFailed))?;

            self.lines.parts.update(f, v);
            Ok(())
        } else {
            let (f, v) = table_searcher
                .find_field_dynamic(self.base - index - 1)
                .ok_or(QpackError::ConnectionError(DecompressionFailed))?;

            self.lines.parts.update(f, v);
            Ok(())
        }
    }

    fn search_post_indexed(&mut self, index: usize) -> Result<(), QpackError> {
        let table_searcher = TableSearcher::new(self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.base + index)
            .ok_or(QpackError::ConnectionError(DecompressionFailed))?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_literal_with_indexing(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), QpackError> {
        let (f, v) = self.get_field_by_name_and_value(
            mid_bit,
            name,
            value,
            ReprPrefixBit::LITERALWITHINDEXING,
        )?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_literal_with_post_indexing(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), QpackError> {
        let (f, v) = self.get_field_by_name_and_value(
            mid_bit,
            name,
            value,
            ReprPrefixBit::LITERALWITHPOSTINDEXING,
        )?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_listeral_with_literal(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), QpackError> {
        let (h, v) = self.get_field_by_name_and_value(
            mid_bit,
            name,
            value,
            ReprPrefixBit::LITERALWITHLITERALNAME,
        )?;
        self.check_field_list_size(&h, &v)?;
        self.lines.parts.update(h, v);
        Ok(())
    }

    fn get_field_by_name_and_value(
        &self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
        repr: ReprPrefixBit,
    ) -> Result<(NameField, String), QpackError> {
        let h = match name {
            Name::Index(index) => {
                if repr == ReprPrefixBit::LITERALWITHINDEXING {
                    let searcher = TableSearcher::new(self.table);
                    if let Some(true) = mid_bit.t {
                        searcher
                            .find_field_name_static(index)
                            .ok_or(QpackError::ConnectionError(DecompressionFailed))?
                    } else {
                        searcher
                            .find_field_name_dynamic(self.base - index - 1)
                            .ok_or(QpackError::ConnectionError(DecompressionFailed))?
                    }
                } else {
                    let searcher = TableSearcher::new(self.table);
                    searcher
                        .find_field_name_dynamic(self.base + index)
                        .ok_or(QpackError::ConnectionError(DecompressionFailed))?
                }
            }
            Name::Literal(octets) => NameField::Other(
                String::from_utf8(octets)
                    .map_err(|_| QpackError::ConnectionError(DecompressionFailed))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| QpackError::ConnectionError(DecompressionFailed))?;
        Ok((h, v))
    }
    pub(crate) fn update_size(&mut self, addition: usize) {
        self.lines.header_size += addition;
    }

    fn check_field_list_size(&mut self, key: &NameField, value: &str) -> Result<(), QpackError> {
        let line_size = field_line_length(key.len(), value.len());
        self.update_size(line_size);
        if self.lines.header_size > self.max_field_section_size {
            Err(QpackError::ConnectionError(DecompressionFailed))
        } else {
            Ok(())
        }
    }
}

fn field_line_length(key_size: usize, value_size: usize) -> usize {
    key_size + value_size + 32
}
