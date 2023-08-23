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

use std::mem::take;
use crate::h3::error::ErrorCode::{QPACK_DECOMPRESSION_FAILED, QPACK_ENCODER_STREAM_ERROR};
use crate::h3::error::H3Error;
use crate::h3::parts::Parts;
use crate::h3::qpack::{Representation, MidBit, ReprPrefixBit, EncoderInstruction, EncoderInstPrefixBit};

use crate::h3::qpack::format::decoder::{EncInstDecoder, InstDecStateHolder, Name, ReprDecoder, ReprDecStateHolder};
use crate::h3::qpack::integer::Integer;
use crate::h3::qpack::table::{DynamicTable, Field, TableSearcher};


struct FiledLines {
    parts: Parts,
    header_size: usize,
}

pub(crate) struct QpackDecoder<'a> {
    field_list_size: usize,
    table: &'a mut DynamicTable,
    repr_holder: ReprDecStateHolder,
    inst_holder: InstDecStateHolder,
    lines: FiledLines,
    base: usize,
    require_insert_count: usize,
}

impl<'a> QpackDecoder<'a> {
    pub(crate) fn new(field_list_size: usize, table: &'a mut DynamicTable) -> Self {
        Self {
            field_list_size,
            table,
            repr_holder: ReprDecStateHolder::new(),
            inst_holder: InstDecStateHolder::new(),
            lines: FiledLines {
                parts: Parts::new(),
                header_size: 0,
            },
            base: 0,
            require_insert_count: 0,
        }
    }

    /// Users can call `decode_ins` multiple times to decode decoder instructions.
    pub(crate) fn decode_ins(&mut self, buf: &[u8]) -> Result<(), H3Error> {
        let mut decoder = EncInstDecoder::new(buf);
        decoder.load(&mut self.inst_holder);
        let mut updater = Updater::new(&mut self.table);
        loop {
            match decoder.decode()? {
                Some(inst) => {
                    match inst {
                        EncoderInstruction::SetCap { capacity } => {
                            updater.update_capacity(capacity)?;
                        }
                        EncoderInstruction::InsertWithIndex { mid_bit, name, value } => {
                            updater.update_table(mid_bit, name, value)?;
                        }
                        EncoderInstruction::InsertWithLiteral { mid_bit, name, value } => {
                            updater.update_table(mid_bit, name, value)?;
                        }
                        EncoderInstruction::Duplicate { index } => {
                            updater.duplicate(index)?;
                        }
                    }
                }
                None => return Result::Ok(())
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
    pub(crate) fn decode_repr(&mut self, buf: &[u8]) -> Result<(), H3Error> {
        let mut decoder = ReprDecoder::new(buf);
        decoder.load(&mut self.repr_holder);

        let mut searcher = Searcher::new(self.field_list_size, &self.table, &mut self.lines);
        loop {
            match decoder.decode()? {
                Some(repr) => {
                    match repr {
                        Representation::FieldSectionPrefix { require_insert_count, signal, delta_base } => {
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
                            //todo:block
                        }
                        Representation::Indexed { mid_bit, index } => {
                            searcher.search(Representation::Indexed { mid_bit, index })?;
                        }
                        Representation::IndexedWithPostIndex { index } => {
                            searcher.search(Representation::IndexedWithPostIndex { index })?;
                        }
                        Representation::LiteralWithIndexing { mid_bit, name, value } => {
                            searcher.search_literal_with_indexing(mid_bit, name, value)?;
                        }
                        Representation::LiteralWithPostIndexing { mid_bit, name, value } => {
                            searcher.search_literal_with_post_indexing(mid_bit, name, value)?;
                        }
                        Representation::LiteralWithLiteralName { mid_bit, name, value } => {
                            searcher.search_listeral_with_literal(mid_bit, name, value)?;
                        }
                    }
                }
                None => return Result::Ok(())
            }
        };
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
    pub(crate) fn finish(&mut self, stream_id: usize, buf: &mut [u8]) -> Result<(Parts, Option<usize>), H3Error> {
        if !self.repr_holder.is_empty() {
            return Err(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED));
        }
        self.lines.header_size = 0;
        if self.require_insert_count > 0 {
            let mut ack = Integer::index(0x80, stream_id, 0x7f);
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
    pub(crate) fn stream_cancel(&mut self, stream_id: usize, buf: &mut [u8]) -> Result<usize, H3Error> {
        let mut ack = Integer::index(0x40, stream_id, 0x3f);
        let size = ack.encode(buf);
        if let Ok(size) = size {
            return Ok(size);
        }
        Err(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))
    }
}

struct Updater<'a> {
    table: &'a mut DynamicTable,
}

impl<'a> Updater<'a> {
    fn new(table: &'a mut DynamicTable) -> Self {
        Self {
            table,
        }
    }

    fn update_capacity(&mut self, capacity: usize) -> Result<(), H3Error> {
        self.table.update_size(capacity);
        Ok(())
    }

    fn update_table(&mut self, mid_bit: MidBit, name: Name, value: Vec<u8>) -> Result<(), H3Error> {
        let (f, v) = self.get_field_by_name_and_value(mid_bit, name, value, self.table.insert_count)?;
        self.table.update(f, v);
        Ok(())
    }

    fn duplicate(&mut self, index: usize) -> Result<(), H3Error> {
        let table_searcher = TableSearcher::new(self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.table.insert_count - index -1)
            .ok_or(H3Error::ConnectionError(QPACK_ENCODER_STREAM_ERROR))?;
        self.table.update(f, v);
        Ok(())
    }

    fn get_field_by_name_and_value(
        &self,
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
        insert_count: usize,
    ) -> Result<(Field, String), H3Error> {
        let h = match name {
            Name::Index(index) => {
                let searcher = TableSearcher::new(self.table);
                if let Some(true) = mid_bit.t {
                    searcher
                        .find_field_name_static(index)
                        .ok_or(H3Error::ConnectionError(QPACK_ENCODER_STREAM_ERROR))?
                } else {
                    searcher
                        .find_field_name_dynamic(insert_count - index - 1)
                        .ok_or(H3Error::ConnectionError(QPACK_ENCODER_STREAM_ERROR))?
                }
            }
            Name::Literal(octets) => Field::Other(
                String::from_utf8(octets)
                    .map_err(|_| H3Error::ConnectionError(QPACK_ENCODER_STREAM_ERROR))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| H3Error::ConnectionError(QPACK_ENCODER_STREAM_ERROR))?;
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

    fn search(&mut self, repr: Representation) -> Result<(), H3Error> {
        match repr {
            Representation::Indexed { mid_bit, index } => {
                self.search_indexed(mid_bit, index)
            }
            Representation::IndexedWithPostIndex { index } => {
                self.search_post_indexed(index)
            }
            _ => {
                Ok(())
            }
        }
    }

    fn search_indexed(&mut self, mid_bit: MidBit, index: usize) -> Result<(), H3Error> {
        let table_searcher = TableSearcher::new(&mut self.table);
        if let Some(true) = mid_bit.t {
            let (f, v) = table_searcher
                .find_field_static(index)
                .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?;

            self.lines.parts.update(f, v);
            return Ok(());
        } else {
            let (f, v) = table_searcher
                .find_field_dynamic(self.base - index-1)
                .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?;

            self.lines.parts.update(f, v);
            return Ok(());
        }
    }

    fn search_post_indexed(&mut self, index: usize) -> Result<(), H3Error> {
        let table_searcher = TableSearcher::new(&mut self.table);
        let (f, v) = table_searcher
            .find_field_dynamic(self.base + index)
            .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        return Ok(());
    }

    fn search_literal_with_indexing(&mut self, mid_bit: MidBit, name: Name, value: Vec<u8>) -> Result<(), H3Error> {
        let (f, v) = self.get_field_by_name_and_value(mid_bit, name, value, ReprPrefixBit::LITERALWITHINDEXING)?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_literal_with_post_indexing(&mut self, mid_bit: MidBit, name: Name, value: Vec<u8>) -> Result<(), H3Error> {
        let (f, v) = self.get_field_by_name_and_value(mid_bit, name, value, ReprPrefixBit::LITERALWITHPOSTINDEXING)?;
        self.check_field_list_size(&f, &v)?;
        self.lines.parts.update(f, v);
        Ok(())
    }

    fn search_listeral_with_literal(&mut self, mid_bit: MidBit, name: Name, value: Vec<u8>) -> Result<(), H3Error> {
        let (h, v) = self.get_field_by_name_and_value(mid_bit, name, value, ReprPrefixBit::LITERALWITHLITERALNAME)?;
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
    ) -> Result<(Field, String), H3Error> {
        let h = match name {
            Name::Index(index) => {
                if repr == ReprPrefixBit::LITERALWITHINDEXING {
                    let searcher = TableSearcher::new(&self.table);
                    if let Some(true) = mid_bit.t {
                        searcher
                            .find_field_name_static(index)
                            .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?
                    } else {
                        searcher
                            .find_field_name_dynamic(self.base - index - 1)
                            .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?
                    }
                } else {
                    let searcher = TableSearcher::new(&self.table);
                    searcher
                        .find_field_name_dynamic(self.base + index)
                        .ok_or(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?
                }
            }
            Name::Literal(octets) => Field::Other(
                String::from_utf8(octets)
                    .map_err(|_| H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))?;
        Ok((h, v))
    }
    pub(crate) fn update_size(&mut self, addition: usize) {
        self.lines.header_size += addition;
    }
    fn check_field_list_size(&mut self, key: &Field, value: &String) -> Result<(), H3Error> {
        let line_size = field_line_length(key.len(), value.len());
        self.update_size(line_size);
        if self.lines.header_size > self.field_list_size {
            Err(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))
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
    use crate::h3::qpack::table::{DynamicTable, Field};
    use crate::h3::qpack::QpackDecoder;
    use crate::test_util::decode;

    const MAX_HEADER_LIST_SIZE: usize = 16 << 20;


    #[test]
    fn ut_qpack_decoder() {
        rfc9204_test_cases();
        test_indexed_static();
        test_indexed_dynamic();
        test_post_indexed_dynamic();
        test_literal_indexing_static();
        test_literal_indexing_dynamic();
        test_literal_post_indexing_dynamic();
        test_literal_with_literal_name();
        test_setcap();
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
                    let text = decode($input).unwrap();
                    assert!($qpack.decode_repr(text.as_slice()).is_ok());
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
                    assert_eq!($map.get($k).unwrap().to_str().unwrap(), $v);
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
                let mut dynamic_table = DynamicTable::with_capacity(4096);

                qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                    "0000510b2f696e6465782e68746d6c",
                    { None, None, Some("/index.html"), None, None },
                    { 0 }
                );
            }
            fn dynamic_table() {
                let mut dynamic_table = DynamicTable::with_capacity(4096);
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                decoder.decode_ins(decode("3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468").unwrap().as_slice());
                qpack_test_case!(
                decoder,
                    "03811011",
                    { Some("www.example.com"), None, Some("/sample/path"), None, None },
                    { 0 }
                );
            }
            fn speculative_insert() {
                let mut dynamic_table = DynamicTable::with_capacity(4096);
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                decoder.decode_ins(decode("4a637573746f6d2d6b65790c637573746f6d2d76616c7565").unwrap().as_slice());
                qpack_test_case!(
                decoder,
                    "028010",
                    { "custom-key"=>"custom-value" },
                    { 0 }
                );
            }
            fn duplicate_instruction_stream_cancellation() {
                let mut dynamic_table = DynamicTable::with_capacity(4096);
                dynamic_table.update(Field::Authority,String::from("www.example.com"));
                dynamic_table.update(Field::Path,String::from("/sample/path"));
                dynamic_table.update(Field::Other(String::from("custom-key")),String::from("custom-value"));
                dynamic_table.ref_count.insert(0, 0); //Acked
                dynamic_table.ref_count.insert(1, 0); //Acked
                dynamic_table.ref_count.insert(2, 0); //Acked
                dynamic_table.known_received_count = 3;
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                decoder.decode_ins(decode("02").unwrap().as_slice());
                qpack_test_case!(
                decoder,
                    "058010c180",
                    { Some("www.example.com"), None, Some("/"), None, None },
                    { "custom-key"=>"custom-value" },
                    { 0 }
                );
            }
            fn dynamic_table_insert_eviction(){
                let mut dynamic_table = DynamicTable::with_capacity(4096);
                dynamic_table.update(Field::Authority,String::from("www.example.com"));
                dynamic_table.update(Field::Path,String::from("/sample/path"));
                dynamic_table.update(Field::Other(String::from("custom-key")),String::from("custom-value"));
                dynamic_table.update(Field::Authority,String::from("www.example.com"));
                dynamic_table.ref_count.insert(0, 0); //Acked
                dynamic_table.ref_count.insert(1, 0); //Acked
                dynamic_table.ref_count.insert(2, 0); //Acked
                dynamic_table.known_received_count = 3;
                let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
                decoder.decode_ins(decode("810d637573746f6d2d76616c756532").unwrap().as_slice());
                qpack_test_case!(
                decoder,
                    "068111",
                    { "custom-key"=>"custom-value2" },
                    { 0 }
                );
            }
        }

        fn test_indexed_static()
        {
            let mut dynamic_table = DynamicTable::with_capacity(4096);

            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                "0000d1",
                { None, Some("GET"), None, None, None },
                { 0 }
            );
        }
        fn test_indexed_dynamic()
        {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            //abs = 0
            dynamic_table.update(Field::Other(String::from("custom-field")), String::from("custom-value"));
            //abs = 1
            dynamic_table.update(Field::Other(String::from("my-field")), String::from("my-value"));
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
        fn test_post_indexed_dynamic()
        {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            //abs = 0
            dynamic_table.update(Field::Other(String::from("custom1-field")), String::from("custom1-value"));
            //abs = 1
            dynamic_table.update(Field::Other(String::from("custom2-field")), String::from("custom2-value"));
            //abs = 2
            dynamic_table.update(Field::Other(String::from("custom3-field")), String::from("custom3-value"));
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
        fn test_literal_indexing_static()
        {
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE,&mut dynamic_table),
                "00007f020d637573746f6d312d76616c7565",
                { None, Some("custom1-value"), None, None, None },
                { 0 }
            );
        }

        fn test_literal_indexing_dynamic()
        {
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            //abs = 0
            dynamic_table.update(Field::Other(String::from("custom-field")), String::from("custom-value"));
            //abs = 1
            dynamic_table.update(Field::Other(String::from("my-field")), String::from("my-value"));
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

        fn test_literal_post_indexing_dynamic()
        {
            // Test index "custom-field"=>"custom-value" in dynamic table
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            //abs = 0
            dynamic_table.update(Field::Other(String::from("custom1-field")), String::from("custom1-value"));
            //abs = 1
            dynamic_table.update(Field::Other(String::from("custom2-field")), String::from("custom2-value"));
            //abs = 2
            dynamic_table.update(Field::Other(String::from("custom3-field")), String::from("custom3-value"));
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

        fn test_literal_with_literal_name()
        {
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            qpack_test_case!(
                QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table),
                "00003706637573746f6d322d76616c75650d637573746f6d312d76616c7565",
                {"custom2-value"=>"custom1-value"},
                {0},
            );
        }

        fn test_setcap()
        {
            let mut dynamic_table = DynamicTable::with_capacity(4096);
            let mut decoder = QpackDecoder::new(MAX_HEADER_LIST_SIZE, &mut dynamic_table);
            let text = decode("3f7f").unwrap();
            decoder.decode_ins(text.as_slice());
            assert_eq!(dynamic_table.capacity(), 158);
        }
    }
}







