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
use crate::h3::qpack::error::ErrorCode::{DecompressionFailed, EncoderStreamError};
use crate::h3::qpack::error::H3errorQpack;
use crate::h3::qpack::{
    DeltaBase, EncoderInstPrefixBit, EncoderInstruction, MidBit, ReprPrefixBit, Representation,
    RequireInsertCount,
};
use std::mem::take;

use crate::h3::qpack::format::decoder::{
    EncInstDecoder, InstDecodeState, Name, ReprDecodeState, ReprDecoder,
};
use crate::h3::qpack::integer::Integer;
use crate::h3::qpack::table::Field::Path;
use crate::h3::qpack::table::{DynamicTable, Field, TableSearcher};

/// An decoder is used to de-compress field in a compression format for efficiently representing
/// HTTP fields that is to be used in HTTP/3. This is a variation of HPACK compression that seeks
/// to reduce head-of-line blocking.
///
/// # Examples(not run)
// ```no_run
// use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
// use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
// use crate::ylong_http::test_util::decode;
// const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//
// // Required content:
// let mut dynamic_table = DynamicTable::with_empty();
// let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//
// //decode instruction
// // convert hex string to dec-array
// let mut inst = decode("3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468").unwrap().as_slice().to_vec();
// decoder.decode_ins(&mut inst);
//
// //decode field section
// // convert hex string to dec-array
// let mut repr = decode("03811011").unwrap().as_slice().to_vec();
// decoder.decode_repr(&mut repr);
//
// ```

pub(crate) struct FiledLines {
    parts: Parts,
    header_size: usize,
}

pub struct QpackDecoder<'a> {
    field_list_size: usize,
    // max header list size
    table: &'a mut DynamicTable,
    // dynamic table
    repr_state: Option<ReprDecodeState>,
    // field decode state
    inst_state: Option<InstDecodeState>,
    // instruction decode state
    lines: FiledLines,
    // field lines, which is used to store the decoded field lines
    base: usize,
    // RFC required, got from field section prefix
    require_insert_count: usize, // RFC required, got from field section prefix
}

impl<'a> QpackDecoder<'a> {

    /// create a new decoder
    /// # Examples(not run)
    ///
//     ```no_run
//     use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
//     use crate::ylong_http::test_util::decode;
//     const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//     // Required content:
//     let mut dynamic_table = DynamicTable::with_empty();
//     let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//     ```
    pub fn new(field_list_size: usize, table: &'a mut DynamicTable) -> Self {
        Self {
            field_list_size,
            table,
            repr_state: None,
            inst_state: None,
            lines: FiledLines {
                parts: Parts::new(),
                header_size: 0,
            },
            base: 0,
            require_insert_count: 0,
        }
    }

    /// Users can call `decode_ins` multiple times to decode decoder instructions.
    /// # Examples(not run)
//     ```no_run
//     use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
//     use crate::ylong_http::test_util::decode;
//     const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//     // Required content:
//     let mut dynamic_table = DynamicTable::with_empty();
//     let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//     //decode instruction
//     // convert hex string to dec-array
//     let mut inst = decode("3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468").unwrap().as_slice().to_vec();
//     decoder.decode_ins(&mut inst);
//     ```
    pub fn decode_ins(&mut self, buf: &[u8]) -> Result<(), H3errorQpack> {
        let mut decoder = EncInstDecoder::new();
        let mut updater = Updater::new(self.table);
        let mut cnt = 0;
        loop {
            match decoder.decode(&buf[cnt..], &mut self.inst_state)? {
                Some(inst) => match inst {
                    (offset, EncoderInstruction::SetCap { capacity }) => {
                        println!("set cap");
                        cnt += offset;
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
                None => return Result::Ok(()),
            }
        }
    }

    /// User call `decoder_repr` once for decoding a complete field section, which start with the `field section prefix`:
    ///  0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// |   Required Insert Count (8+)  |
    /// +---+---------------------------+
    /// | S |      Delta Base (7+)      |
    /// +---+---------------------------+
    /// |      Encoded Field Lines    ...
    /// +-------------------------------+
    /// # Examples(not run)
//     ```no_run
//     use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
//     use crate::ylong_http::test_util::decode;
//     const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//     // Required content:
//     let mut dynamic_table = DynamicTable::with_empty();
//     let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//     //decode field section
//     // convert hex string to dec-array
//     let mut repr = decode("03811011").unwrap().as_slice().to_vec();
//     decoder.decode_repr(&mut repr);
//     ```
    pub fn decode_repr(&mut self, buf: &[u8]) -> Result<(), H3errorQpack> {
        let mut decoder = ReprDecoder::new();
        let mut searcher = Searcher::new(self.field_list_size, self.table, &mut self.lines);
        let mut cnt = 0;
        loop {
            match decoder.decode(&buf[cnt..], &mut self.repr_state)? {
                Some((
                    offset,
                    Representation::FieldSectionPrefix {
                        require_insert_count,
                        signal,
                        delta_base,
                    },
                )) => {
                    cnt += offset;
                    if require_insert_count.0 == 0 {
                        self.require_insert_count = 0;
                    } else {
                        let max_entries = self.table.max_entries();
                        let full_range = 2 * max_entries;
                        let max_value = self.table.insert_count + max_entries;
                        let max_wrapped = (max_value / full_range) * full_range;
                        self.require_insert_count = max_wrapped + require_insert_count.0 - 1;
                        if self.require_insert_count > max_value {
                            self.require_insert_count -= full_range;
                        }
                    }
                    if signal {
                        self.base = self.require_insert_count - delta_base.0 - 1;
                    } else {
                        self.base = self.require_insert_count + delta_base.0;
                    }
                    searcher.base = self.base;
                    if self.require_insert_count > self.table.insert_count {
                        //todo:block
                    }
                }
                Some((offset, Representation::Indexed { mid_bit, index })) => {
                    cnt += offset;
                    searcher.search(Representation::Indexed { mid_bit, index })?;
                }
                Some((offset, Representation::IndexedWithPostIndex { index })) => {
                    cnt += offset;
                    searcher.search(Representation::IndexedWithPostIndex { index })?;
                }
                Some((
                    offset,
                    Representation::LiteralWithIndexing {
                        mid_bit,
                        name,
                        value,
                    },
                )) => {
                    println!("offset:{}", offset);
                    cnt += offset;
                    searcher.search_literal_with_indexing(mid_bit, name, value)?;
                }
                Some((
                    offset,
                    Representation::LiteralWithPostIndexing {
                        mid_bit,
                        name,
                        value,
                    },
                )) => {
                    cnt += offset;
                    searcher.search_literal_with_post_indexing(mid_bit, name, value)?;
                }
                Some((
                    offset,
                    Representation::LiteralWithLiteralName {
                        mid_bit,
                        name,
                        value,
                    },
                )) => {
                    cnt += offset;
                    searcher.search_listeral_with_literal(mid_bit, name, value)?;
                }

                None => {
                    return Result::Ok(());
                }
            }
        }
    }

    /// Users call `finish` to stop decoding a field section. And send an `Section Acknowledgment` to encoder:
    /// After processing an encoded field section whose declared Required Insert Count is not zero,
    /// the decoder emits a Section Acknowledgment instruction. The instruction starts with the
    /// '1' 1-bit pattern, followed by the field section's associated stream ID encoded as
    /// a 7-bit prefix integer
    ///  0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 1 |      Stream ID (7+)       |
    /// +---+---------------------------+
    /// # Examples(not run)
//     ```no_run
//     use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
//     use crate::ylong_http::test_util::decode;
//     const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//     // Required content:
//     let mut dynamic_table = DynamicTable::with_empty();
//     let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//     //decode field section
//     // convert hex string to dec-array
//     let mut repr = decode("03811011").unwrap().as_slice().to_vec();
//     decoder.decode_repr(&mut repr);
//     //finish
//     let mut qpack_decoder_buf = [0u8;20];
//     decoder.finish(1,&mut qpack_decoder_buf);
//     ```
    pub fn finish(
        &mut self,
        stream_id: usize,
        buf: &mut [u8],
    ) -> Result<(Parts, Option<usize>), H3errorQpack> {
        if self.repr_state.is_some() {
            return Err(H3errorQpack::ConnectionError(DecompressionFailed));
        }
        self.lines.header_size = 0;
        if self.require_insert_count > 0 {
            let ack = Integer::index(0x80, stream_id, 0x7f);
            let size = ack.encode(buf);
            if let Ok(size) = size {
                return Ok((take(&mut self.lines.parts), Some(size)));
            }
        }
        Ok((take(&mut self.lines.parts), None))
    }

    /// Users call `stream_cancel` to stop cancel a stream. And send an `Stream Cancellation` to encoder:
    /// When a stream is reset or reading is abandoned, the decoder emits a Stream Cancellation
    /// instruction. The instruction starts with the '01' 2-bit pattern,
    /// followed by the stream ID of the affected stream encoded as a 6-bit prefix integer.
    ///  0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 1 |     Stream ID (6+)    |
    /// +---+---+-----------------------+
    /// # Examples(not run)
//     ```no_run
//     use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use crate::ylong_http::h3::qpack::decoder::QpackDecoder;
//     use crate::ylong_http::test_util::decode;
//     const MAX_HEADER_LIST_SIZE: usize = 16 << 20;
//     // Required content:
//     let mut dynamic_table = DynamicTable::with_empty();
//     let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
//     //decode field section
//     // convert hex string to dec-array
//     let mut repr = decode("03811011").unwrap().as_slice().to_vec();
//     decoder.decode_repr(&mut repr);
//     //stream_cancel
//     let mut qpack_decoder_buf = [0u8;20];
//     decoder.stream_cancel(1,&mut qpack_decoder_buf);
//     ```
    pub fn stream_cancel(
        &mut self,
        stream_id: usize,
        buf: &mut [u8],
    ) -> Result<usize, H3errorQpack> {
        let ack = Integer::index(0x40, stream_id, 0x3f);
        let size = ack.encode(buf);
        if let Ok(size) = size {
            return Ok(size);
        }
        Err(H3errorQpack::ConnectionError(DecompressionFailed))
    }
}

struct Updater<'a> {
    table: &'a mut DynamicTable,
}

impl<'a> Updater<'a> {
    fn new(table: &'a mut DynamicTable) -> Self {
        Self { table }
    }

    fn update_capacity(&mut self, capacity: usize) -> Result<(), H3errorQpack> {
        self.table.update_size(capacity);
        Ok(())
    }

    fn update_table(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), H3errorQpack> {
        let (f, v) =
            self.get_field_by_name_and_value(mid_bit, name, value, self.table.insert_count)?;
        self.table.update(f, v);
        Ok(())
    }

    fn duplicate(&mut self, index: usize) -> Result<(), H3errorQpack> {
        let table_searcher = TableSearcher::new(self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.table.insert_count - index - 1)
            .ok_or(H3errorQpack::ConnectionError(EncoderStreamError))?;
        self.table.update(f, v);
        Ok(())
    }

    fn get_field_by_name_and_value(
        &self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
        insert_count: usize,
    ) -> Result<(Field, String), H3errorQpack> {
        let h = match name {
            Name::Index(index) => {
                let searcher = TableSearcher::new(self.table);
                if let Some(true) = mid_bit.t {
                    searcher
                        .find_field_name_static(index)
                        .ok_or(H3errorQpack::ConnectionError(EncoderStreamError))?
                } else {
                    searcher
                        .find_field_name_dynamic(insert_count - index - 1)
                        .ok_or(H3errorQpack::ConnectionError(EncoderStreamError))?
                }
            }
            Name::Literal(octets) => Field::Other(
                String::from_utf8(octets)
                    .map_err(|_| H3errorQpack::ConnectionError(EncoderStreamError))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| H3errorQpack::ConnectionError(EncoderStreamError))?;
        Ok((h, v))
    }
}

struct Searcher<'a> {
    field_list_size: usize,
    table: &'a DynamicTable,
    lines: &'a mut FiledLines,
    base: usize,
}

impl<'a> Searcher<'a> {
    fn new(field_list_size: usize, table: &'a DynamicTable, lines: &'a mut FiledLines) -> Self {
        Self {
            field_list_size,
            table,
            lines,
            base: 0,
        }
    }

    fn search(&mut self, repr: Representation) -> Result<(), H3errorQpack> {
        match repr {
            Representation::Indexed { mid_bit, index } => self.search_indexed(mid_bit, index),
            Representation::IndexedWithPostIndex { index } => self.search_post_indexed(index),
            _ => Ok(()),
        }
    }

    fn search_indexed(&mut self, mid_bit: MidBit, index: usize) -> Result<(), H3errorQpack> {
        let table_searcher = TableSearcher::new(self.table);
        if let Some(true) = mid_bit.t {
            let (f, v) = table_searcher
                .find_field_static(index)
                .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?;

            self.lines.parts.update(f, v);
            Ok(())
        } else {
            let (f, v) = table_searcher
                .find_field_dynamic(self.base - index - 1)
                .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?;

            self.lines.parts.update(f, v);
            Ok(())
        }
    }

    fn search_post_indexed(&mut self, index: usize) -> Result<(), H3errorQpack> {
        let table_searcher = TableSearcher::new(self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.base + index)
            .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_literal_with_indexing(
        &mut self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), H3errorQpack> {
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
    ) -> Result<(), H3errorQpack> {
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
    ) -> Result<(), H3errorQpack> {
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
    ) -> Result<(Field, String), H3errorQpack> {
        let h = match name {
            Name::Index(index) => {
                if repr == ReprPrefixBit::LITERALWITHINDEXING {
                    let searcher = TableSearcher::new(self.table);
                    if let Some(true) = mid_bit.t {
                        searcher
                            .find_field_name_static(index)
                            .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?
                    } else {
                        searcher
                            .find_field_name_dynamic(self.base - index - 1)
                            .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?
                    }
                } else {
                    let searcher = TableSearcher::new(self.table);
                    searcher
                        .find_field_name_dynamic(self.base + index)
                        .ok_or(H3errorQpack::ConnectionError(DecompressionFailed))?
                }
            }
            Name::Literal(octets) => Field::Other(
                String::from_utf8(octets)
                    .map_err(|_| H3errorQpack::ConnectionError(DecompressionFailed))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| H3errorQpack::ConnectionError(DecompressionFailed))?;
        Ok((h, v))
    }
    pub(crate) fn update_size(&mut self, addition: usize) {
        self.lines.header_size += addition;
    }
    fn check_field_list_size(&mut self, key: &Field, value: &String) -> Result<(), H3errorQpack> {
        let line_size = field_line_length(key.len(), value.len());
        self.update_size(line_size);
        if self.lines.header_size > self.field_list_size {
            Err(H3errorQpack::ConnectionError(DecompressionFailed))
        } else {
            Ok(())
        }
    }
}

fn field_line_length(key_size: usize, value_size: usize) -> usize {
    key_size + value_size + 32
}

#[cfg(test)]
mod ut_qpack_decoder {
    use crate::h3::qpack::format::decoder::ReprDecodeState;
    use crate::h3::qpack::table::{DynamicTable, Field};
    use crate::h3::qpack::QpackDecoder;
    use crate::util::test_util::decode;

    const MAX_HEADER_LIST_SIZE: usize = 16 << 20;

    #[test]
    fn ut_qpack_decoder() {
        rfc9204_test_cases();
        test_need_more();
        test_indexed_static();
        test_indexed_dynamic();
        test_post_indexed_dynamic();
        test_literal_indexing_static();
        test_literal_indexing_dynamic();
        test_literal_post_indexing_dynamic();
        test_literal_with_literal_name();
        test_setcap();
        decode_long_field();

        fn get_state(state: &Option<ReprDecodeState>) {
            match state {
                Some(ReprDecodeState::FiledSectionPrefix(_)) => {
                    println!("FiledSectionPrefix");
                }
                Some(ReprDecodeState::ReprIndex(_)) => {
                    println!("Indexed");
                }
                Some(ReprDecodeState::ReprValueString(_)) => {
                    println!("ReprValueString");
                }
                Some(ReprDecodeState::ReprNameAndValue(_)) => {
                    println!("ReprNameAndValue");
                }
                None => {
                    println!("None");
                }
            }
        }
        macro_rules! check_pseudo {
            (
                $pseudo: expr,
                { $a: expr, $m: expr, $p: expr, $sc: expr, $st: expr } $(,)?
            ) => {
                assert_eq!($pseudo.authority(), $a);
                assert_eq!($pseudo.method(), $m);
                assert_eq!($pseudo.path(), $p);
                assert_eq!($pseudo.scheme(), $sc);
                assert_eq!($pseudo.status(), $st);
            };
        }

        macro_rules! get_parts {
            ($qpack: expr $(, $input: literal)*) => {{
                $(
                    let text = decode($input).unwrap().as_slice().to_vec();
                    assert!($qpack.decode_repr(&text).is_ok());
                )*
                let mut ack = [0u8; 20];
                match $qpack.finish(1,&mut ack) {
                    Ok((parts,_)) => parts,
                    Err(_) => panic!("QpackDecoder::finish() failed!"),
                }
            }};
        }
        macro_rules! check_map {
            ($map: expr, { $($(,)? $k: literal => $v: literal)* } $(,)?) => {
                $(
                    assert_eq!($map.get($k).unwrap().to_string().unwrap(), $v);
                )*
            }
        }
        macro_rules! qpack_test_case {
            (
                $qpack: expr $(, $input: literal)*,
                { $a: expr, $m: expr, $p: expr, $sc: expr, $st: expr },
                { $size: expr $(, $($k2: literal)? $($k3: ident)? => $v2: literal)* } $(,)?
            ) => {
                let mut _qpack = $qpack;
                let (pseudo, _) = get_parts!(_qpack $(, $input)*).into_parts();
                check_pseudo!(pseudo, { $a, $m, $p, $sc, $st });
            };
            (
                $qpack: expr $(, $input: literal)*,
                { $($(,)? $k1: literal => $v1: literal)* },
                { $size: expr $(, $($k2: literal)? $($k3: ident)? => $v2: literal)* } $(,)?
            ) => {
                let mut _qpack = $qpack;
                let (_, map) = get_parts!(_qpack $(, $input)*).into_parts();
                check_map!(map, { $($k1 => $v1)* });
            };
            (
                $hpack: expr $(, $input: literal)*,
                { $a: expr, $m: expr, $p: expr, $sc: expr, $st: expr },
                { $($(,)? $k1: literal => $v1: literal)* },
                { $size: expr $(, $($k2: literal)? $($k3: ident)? => $v2: literal)* } $(,)?
            ) => {
                let mut _hpack = $hpack;
                let (pseudo, map) = get_parts!(_hpack $(, $input)*).into_parts();
                check_pseudo!(pseudo, { $a, $m, $p, $sc, $st });
                check_map!(map, { $($k1 => $v1)* });
            };
        }

        fn rfc9204_test_cases() {
            literal_field_line_with_name_reference();
            dynamic_table();
            speculative_insert();
            duplicate_instruction_stream_cancellation();
            dynamic_table_insert_eviction();
            fn literal_field_line_with_name_reference() {
                println!("run literal_field_line_with_name_reference");
                let mut dynamic_table = DynamicTable::with_empty();
                dynamic_table.update_size(4096);
                let decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                qpack_test_case!(
                decoder,
                    "0000510b2f696e6465782e68746d6c",
                    { None, None, Some("/index.html"), None, None },
                    { 0 }
                );
                println!("passed");
            }
            fn dynamic_table() {
                println!("dynamic_table");
                let mut dynamic_table = DynamicTable::with_empty();
                dynamic_table.update_size(4096);
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                let ins =
                    decode("3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468")
                        .unwrap()
                        .as_slice()
                        .to_vec();
                let _ = decoder.decode_ins(&ins);
                get_state(&decoder.repr_state);
                qpack_test_case!(
                decoder,
                    "03811011",
                    { Some("www.example.com"), None, Some("/sample/path"), None, None },
                    { 0 }
                );
            }
            fn speculative_insert() {
                let mut dynamic_table = DynamicTable::with_empty();
                dynamic_table.update_size(4096);
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                let ins = decode("4a637573746f6d2d6b65790c637573746f6d2d76616c7565")
                    .unwrap()
                    .as_slice()
                    .to_vec();
                let _ = decoder.decode_ins(&ins);
                qpack_test_case!(
                decoder,
                    "028010",
                    { "custom-key"=>"custom-value" },
                    { 0 }
                );
            }
            fn duplicate_instruction_stream_cancellation() {
                let mut dynamic_table = DynamicTable::with_empty();
                dynamic_table.update_size(4096);
                dynamic_table.update(Field::Authority, String::from("www.example.com"));
                dynamic_table.update(Field::Path, String::from("/sample/path"));
                dynamic_table.update(
                    Field::Other(String::from("custom-key")),
                    String::from("custom-value"),
                );
                dynamic_table.known_received_count = 3;
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                let ins = decode("02").unwrap().as_slice().to_vec();
                let _ = decoder.decode_ins(&ins);
                qpack_test_case!(
                decoder,
                    "058010c180",
                    { Some("www.example.com"), None, Some("/"), None, None },
                    { "custom-key"=>"custom-value" },
                    { 0 }
                );
            }
            fn dynamic_table_insert_eviction() {
                let mut dynamic_table = DynamicTable::with_empty();
                dynamic_table.update_size(4096);
                dynamic_table.update(Field::Authority, String::from("www.example.com"));
                dynamic_table.update(Field::Path, String::from("/sample/path"));
                dynamic_table.update(
                    Field::Other(String::from("custom-key")),
                    String::from("custom-value"),
                );
                dynamic_table.update(Field::Authority, String::from("www.example.com"));
                dynamic_table.known_received_count = 3;
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                let ins = decode("810d637573746f6d2d76616c756532")
                    .unwrap()
                    .as_slice()
                    .to_vec();
                let _ = decoder.decode_ins(&ins);
                qpack_test_case!(
                decoder,
                    "068111",
                    { "custom-key"=>"custom-value2" },
                    { 0 }
                );
            }
        }

        fn test_need_more() {
            println!("test_need_more");
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
            let text = decode("00").unwrap().as_slice().to_vec(); //510b2f696e6465782e68746d6c
            println!("text={:?}", text);
            let _ = decoder.decode_repr(&text);
            get_state(&decoder.repr_state);
            let text2 = decode("00510b2f696e6465782e68746d6c")
                .unwrap()
                .as_slice()
                .to_vec();
            println!("text2={:?}", text2);
            let _ = decoder.decode_repr(&text2);
        }

        fn test_indexed_static() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);

            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                "0000d1",
                { None, Some("GET"), None, None, None },
                { 0 }
            );
        }
        fn test_indexed_dynamic() {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            //abs = 0
            dynamic_table.update(
                Field::Other(String::from("custom-field")),
                String::from("custom-value"),
            );
            //abs = 1
            dynamic_table.update(
                Field::Other(String::from("my-field")),
                String::from("my-value"),
            );
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                //require_insert_count=2, signal=false, delta_base=0
                //so base=2
                //rel_index=1 (abs=2(base)-1-1=0)
                "030081",
                {"custom-field"=>"custom-value"},
                { 0 }
            );
        }
        fn test_post_indexed_dynamic() {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            //abs = 0
            dynamic_table.update(
                Field::Other(String::from("custom1-field")),
                String::from("custom1-value"),
            );
            //abs = 1
            dynamic_table.update(
                Field::Other(String::from("custom2-field")),
                String::from("custom2-value"),
            );
            //abs = 2
            dynamic_table.update(
                Field::Other(String::from("custom3-field")),
                String::from("custom3-value"),
            );
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                //require_insert_count=3, signal=true, delta_base=2
                //so base = 3-2-1 = 0
                //rel_index=1 (abs=0(base)+1=1)
                "048211",
                {"custom2-field"=>"custom2-value"},
                { 0 }
            );
        }
        fn test_literal_indexing_static() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                "00007f020d637573746f6d312d76616c7565",
                { None, Some("custom1-value"), None, None, None },
                { 0 }
            );
        }

        fn test_literal_indexing_dynamic() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            //abs = 0
            dynamic_table.update(
                Field::Other(String::from("custom-field")),
                String::from("custom-value"),
            );
            //abs = 1
            dynamic_table.update(
                Field::Other(String::from("my-field")),
                String::from("my-value"),
            );
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                //require_insert_count=2, signal=false, delta_base=0
                //so base=2
                //rel_index=1 (abs=2(base)-1-1=0)
                "0300610d637573746f6d312d76616c7565",
                {"custom-field"=>"custom1-value"},
                { 0 }
            );
        }

        fn test_literal_post_indexing_dynamic() {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            //abs = 0
            dynamic_table.update(
                Field::Other(String::from("custom1-field")),
                String::from("custom1-value"),
            );
            //abs = 1
            dynamic_table.update(
                Field::Other(String::from("custom2-field")),
                String::from("custom2-value"),
            );
            //abs = 2
            dynamic_table.update(
                Field::Other(String::from("custom3-field")),
                String::from("custom3-value"),
            );
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                //require_insert_count=3, signal=true, delta_base=2
                //so base = 3-2-1 = 0
                //rel_index=1 (abs=0(base)+1=1)
                "0482010d637573746f6d312d76616c7565",
                {"custom2-field"=>"custom1-value"},
                { 0 }
            );
        }

        fn test_literal_with_literal_name() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table),
                "00003706637573746f6d322d76616c75650d637573746f6d312d76616c7565",
                {"custom2-value"=>"custom1-value"},
                {0},
            );
        }

        fn test_setcap() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
            let ins = decode("3fbd01").unwrap().as_slice().to_vec();
            let _ = decoder.decode_ins(&ins);
            assert_eq!(decoder.table.capacity(), 220);
        }

        fn decode_long_field() {
            let mut dynamic_table = DynamicTable::with_empty();
            dynamic_table.update_size(4096);
            let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
            let repr = decode("ffffff01ffff01037fffff01")
                .unwrap()
                .as_slice()
                .to_vec();
            let _ = decoder.decode_repr(&repr);
            assert_eq!(decoder.base, 32382);
        }
    }
}
