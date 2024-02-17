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
use crate::h3::qpack::format::encoder::{
    DecInstDecoder, InstDecodeState, PartsIter, ReprEncodeState, SetCap,
};
use crate::h3::qpack::format::ReprEncoder;
use crate::h3::qpack::integer::{Integer, IntegerEncoder};
use crate::h3::qpack::table::{DynamicTable, Field};
use crate::h3::qpack::{DecoderInstruction, PrefixMask};
use std::collections::{HashMap, VecDeque};

/// An encoder is used to compress field in a compression format for efficiently representing
/// HTTP fields that is to be used in HTTP/3. This is a variation of HPACK compression that seeks
/// to reduce head-of-line blocking.
///
/// # Examples
// ```no_run
// use crate::ylong_http::h3::qpack::encoder::QpackEncoder;
// use crate::ylong_http::h3::parts::Parts;
// use crate::ylong_http::h3::qpack::table::{DynamicTable, Field};
// use crate::ylong_http::test_util::decode;
//
//
//
// // the (field, value) is: ("custom-key", "custom-value2")
// // Required content:
// let mut encoder_buf = [0u8; 1024]; // QPACK stream providing control commands.
// let mut stream_buf = [0u8; 1024]; // Field section encoded in QPACK format.
// let mut encoder_cur = 0; // index of encoder_buf.
// let mut stream_cur = 0; // index of stream_buf.
// let mut table = DynamicTable::with_empty();
//
// // create a new encoder.
// let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
//
// // set dynamic table capacity.
// encoder_cur += encoder.set_capacity(220, &mut encoder_buf[encoder_buf..]);
//
// // set field section.
// let mut field = Parts::new();
// field.update(Field::Other(String::from("custom-key")), String::from("custom-value"));
// encoder.set_parts(field);
//
// // encode field section.
// let (cur1, cur2, _) = encoder.encode(&mut encoder_buf[encoder_cur..], &mut stream_buf[stream_cur..]);
// encoder_cur += cur1;
// stream_cur += cur2;
//
// assert_eq!(stream_buf[..encoder_cur].to_vec().as_slice(), decode("028010").unwrap().as_slice());
// assert_eq!(stream_buf[..stream_cur].to_vec().as_slice(), decode("4a637573746f6d2d6b65790c637573746f6d2d76616c7565").unwrap().as_slice());
//
//
// ```

pub struct QpackEncoder<'a> {
    table: &'a mut DynamicTable,
    // Headers to be encode.
    field_iter: Option<PartsIter>,
    // save the state of encoding field.
    field_state: Option<ReprEncodeState>,
    // save the state of decoding instructions.
    inst_state: Option<InstDecodeState>,
    // list of fields to be inserted.
    insert_list: VecDeque<(Field, String)>,
    insert_length: usize,
    // `RFC`: the number of insertions that the decoder needs to receive before it can decode the field section.
    required_insert_count: usize,

    stream_id: usize,
    // allow reference to the inserting field default is false.
    allow_post: bool,
    // RFC9204-2.1.1.1. if index<draining_index, then execute Duplicate.
    draining_index: usize,
}

impl<'a> QpackEncoder<'a> {

    /// create a new encoder.
    /// #Examples
//     ```no_run
//     use ylong_http::h3::qpack::encoder::QpackEncoder;
//     use ylong_http::h3::qpack::table::DynamicTable;
//     let mut encoder_buf = [0u8; 1024]; // QPACK stream providing control commands.
//     let mut stream_buf = [0u8; 1024]; // Field section encoded in QPACK format.
//     let mut encoder_cur = 0; // index of encoder_buf.
//     let mut stream_cur = 0; // index of stream_buf.
//     let mut table = DynamicTable::with_empty();
//
//     // create a new encoder.
//     let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
//     ```
    pub fn new(
        table: &'a mut DynamicTable,
        stream_id: usize,
        allow_post: bool,
        draining_index: usize,
    ) -> QpackEncoder {
        Self {
            table,
            field_iter: None,
            field_state: None,
            inst_state: None,
            insert_list: VecDeque::new(),
            insert_length: 0,
            required_insert_count: 0,
            stream_id,
            allow_post,
            draining_index,
        }
    }

    /// Set the maximum dynamic table size.
    /// # Examples
    /// ```no_run
    /// use ylong_http::h3::qpack::encoder::QpackEncoder;
    /// use ylong_http::h3::qpack::table::DynamicTable;
    /// let mut encoder_buf = [0u8; 1024]; // QPACK stream providing control commands.
    /// let mut stream_buf = [0u8; 1024]; // Field section encoded in QPACK format.
    /// let mut encoder_cur = 0; // index of encoder_buf.
    /// let mut stream_cur = 0; // index of stream_buf.
    /// let mut table = DynamicTable::with_empty();
    /// let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
    /// let mut encoder_cur = encoder.set_capacity(220, &mut encoder_buf[..]);
    /// ```
    pub fn set_capacity(&mut self, max_size: usize, encoder_buf: &mut [u8]) -> usize {
        self.table.update_size(max_size);
        if let Ok(cur) = SetCap::new(max_size).encode(&mut encoder_buf[..]) {
            return cur;
        }
        0
    }


    /// Set the field section to be encoded.
    /// # Examples
//     ```no_run
//     use ylong_http::h3::qpack::encoder::QpackEncoder;
//     use ylong_http::h3::parts::Parts;
//     use ylong_http::h3::qpack::table::{DynamicTable, Field};
//     let mut table = DynamicTable::with_empty();
//     let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
//     let mut parts = Parts::new();
//     parts.update(Field::Other(String::from("custom-key")), String::from("custom-value"));
//     encoder.set_parts(parts);
//     ```
    pub fn set_parts(&mut self, parts: Parts) {
        self.field_iter = Some(PartsIter::new(parts));
    }

    fn ack(&mut self, stream_id: usize) -> Result<Option<DecoderInst>, H3errorQpack> {
        assert_eq!(stream_id, self.stream_id);

        if self.table.known_received_count < self.required_insert_count {
            self.table.known_received_count = self.required_insert_count;
        } else {
            return Err(H3errorQpack::ConnectionError(DecoderStreamError));
        }

        Ok(Some(DecoderInst::Ack))
    }

    /// Users can call `decode_ins` multiple times to decode decoder instructions.
    /// # Return
    /// `Ok(None)` means that the decoder instruction is not complete.
    /// # Examples
//     ```no_run
//     use ylong_http::h3::qpack::encoder::QpackEncoder;
//     use ylong_http::h3::parts::Parts;
//     use ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use ylong_http::test_util::decode;
//     let mut table = DynamicTable::with_empty();
//     let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
//     let _ = encoder.decode_ins(&mut decode("80").unwrap().as_slice());
//     ```
    pub fn decode_ins(&mut self, buf: &[u8]) -> Result<Option<DecoderInst>, H3errorQpack> {
        let mut decoder = DecInstDecoder::new(buf);

        match decoder.decode(&mut self.inst_state)? {
            Some(DecoderInstruction::Ack { stream_id }) => self.ack(stream_id),
            //todo: stream cancel
            Some(DecoderInstruction::StreamCancel { stream_id }) => {
                assert_eq!(stream_id, self.stream_id);
                Ok(Some(DecoderInst::StreamCancel))
            }
            //todo: insert count increment
            Some(DecoderInstruction::InsertCountIncrement { increment }) => {
                self.table.known_received_count += increment;
                Ok(Some(DecoderInst::InsertCountIncrement))
            }
            None => Ok(None),
        }
    }

    fn get_prefix(&self, prefix_buf: &mut [u8]) -> usize {
        let mut cur_prefix = 0;
        let mut wire_ric = 0;
        if self.required_insert_count != 0 {
            wire_ric = self.required_insert_count % (2 * self.table.max_entries()) + 1;
        }

        cur_prefix += Integer::index(0x00, wire_ric, 0xff)
            .encode(&mut prefix_buf[..])
            .unwrap_or(0);
        let base = self.table.insert_count;
        println!("base: {}", base);
        println!("required_insert_count: {}", self.required_insert_count);
        if base >= self.required_insert_count {
            cur_prefix += Integer::index(0x00, base - self.required_insert_count, 0x7f)
                .encode(&mut prefix_buf[cur_prefix..])
                .unwrap_or(0);
        } else {
            cur_prefix += Integer::index(0x80, self.required_insert_count - base - 1, 0x7f)
                .encode(&mut prefix_buf[cur_prefix..])
                .unwrap_or(0);
        }
        cur_prefix
    }
    /// Users can call `encode` multiple times to encode multiple complete field sections.
    /// # Examples
//     ```no_run
//     use ylong_http::h3::qpack::encoder::QpackEncoder;
//     use ylong_http::h3::parts::Parts;
//     use ylong_http::h3::qpack::table::{DynamicTable, Field};
//     use ylong_http::test_util::decode;
//     let mut encoder_buf = [0u8; 1024]; // QPACK stream providing control commands.
//     let mut stream_buf = [0u8; 1024]; // Field section encoded in QPACK format.
//     let mut encoder_cur = 0; // index of encoder_buf.
//     let mut stream_cur = 0; // index of stream_buf.
//     let mut table = DynamicTable::with_empty();
//     let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
//     let mut parts = Parts::new();
//     parts.update(Field::Other(String::from("custom-key")), String::from("custom-value"));
//     encoder.set_parts(parts);
//     let (cur1, cur2, _) = encoder.encode(&mut encoder_buf[encoder_cur..], &mut stream_buf[stream_cur..]);
//     encoder_cur += cur1;
//     stream_cur += cur2;
//     ```
    pub fn encode(
        &mut self,
        encoder_buf: &mut [u8], //instructions encoded results
        stream_buf: &mut [u8],  //headers encoded results
    ) -> (usize, usize, Option<([u8; 1024], usize)>) {
        let (mut cur_encoder, mut cur_stream) = (0, 0);
        if self.is_finished() {
            // denote an end of field section
            // self.stream_reference.push_back(None);
            //todo: size of prefix_buf
            let mut prefix_buf = [0u8; 1024];
            let cur_prefix = self.get_prefix(&mut prefix_buf[0..]);
            for (field, value) in self.insert_list.iter() {
                self.table.update(field.clone(), value.clone());
            }
            (cur_encoder, cur_stream, Some((prefix_buf, cur_prefix)))
        } else {
            let mut encoder = ReprEncoder::new(
                self.table,
                self.draining_index,
                self.allow_post,
                &mut self.insert_length,
            );
            (cur_encoder, cur_stream) = encoder.encode(
                &mut self.field_iter,
                &mut self.field_state,
                &mut encoder_buf[0..],
                &mut stream_buf[0..],
                &mut self.insert_list,
                &mut self.required_insert_count,
            );
            (cur_encoder, cur_stream, None)
        }
    }

    /// Check the previously set `Parts` if encoding is complete.
    pub(crate) fn is_finished(&self) -> bool {
        self.field_iter.is_none() && self.field_state.is_none()
    }
}

pub enum DecoderInst {
    Ack,
    StreamCancel,
    InsertCountIncrement,
}

#[cfg(test)]
mod ut_qpack_encoder {
    use crate::h3::parts::Parts;
    use crate::h3::qpack::encoder;
    use crate::h3::qpack::encoder::QpackEncoder;
    use crate::h3::qpack::table::{DynamicTable, Field};
    use crate::util::test_util::decode;
    macro_rules! qpack_test_cases {
            ($enc: expr,$encoder_buf:expr,$encoder_cur:expr, $len: expr, $res: literal,$encoder_res: literal, $size: expr, { $($h: expr, $v: expr $(,)?)*} $(,)?) => {
                let mut _encoder = $enc;
                let mut stream_buf = [0u8; $len];
                let mut stream_cur = 0;
                $(
                    let mut parts = Parts::new();
                    parts.update($h, $v);
                    _encoder.set_parts(parts);
                    let (cur1,cur2,_) = _encoder.encode(&mut $encoder_buf[$encoder_cur..],&mut stream_buf[stream_cur..]);
                    $encoder_cur += cur1;
                    stream_cur += cur2;
                )*
                let (cur1, cur2, prefix) = _encoder.encode(&mut $encoder_buf[$encoder_cur..],&mut stream_buf[stream_cur..]);
                $encoder_cur += cur1;
                stream_cur += cur2;
                if let Some((prefix_buf,cur_prefix)) = prefix{
                    stream_buf.copy_within(0..stream_cur,cur_prefix);
                    stream_buf[..cur_prefix].copy_from_slice(&prefix_buf[..cur_prefix]);
                    stream_cur += cur_prefix;
                }
                println!("stream_buf: {:#?}",stream_buf);
                let result = decode($res).unwrap();
                if let Some(res) = decode($encoder_res){
                    assert_eq!($encoder_buf[..$encoder_cur].to_vec().as_slice(), res.as_slice());
                }
                assert_eq!(stream_cur, $len);
                assert_eq!(stream_buf.as_slice(), result.as_slice());
                assert_eq!(_encoder.table.size(), $size);
            }
        }
    #[test]
    /// The encoder sends an encoded field section containing a literal representation of a field
    /// with a static name reference.
    fn literal_field_line_with_name_reference() {
        println!("literal_field_line_with_name_reference");
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, false, 0);
        let mut encoder_cur = encoder.set_capacity(0, &mut encoder_buf[..]);
        qpack_test_cases!(
            encoder,
            encoder_buf,
            encoder_cur,
            15, "0000510b2f696e6465782e68746d6c",
            "20",
            0,
            {
                Field::Path,
                String::from("/index.html"),
            },
        );
    }

    #[test]
    ///The encoder sets the dynamic table capacity, inserts a header with a dynamic name
    /// reference, then sends a potentially blocking, encoded field section referencing
    /// this new entry. The decoder acknowledges processing the encoded field section,
    /// which implicitly acknowledges all dynamic table insertions up to the Required
    /// Insert Count.
    fn dynamic_table() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 0);
        let mut encoder_cur = encoder.set_capacity(220, &mut encoder_buf[..]);
        qpack_test_cases!(
            encoder,
            encoder_buf,
            encoder_cur,
            4, "03811011",
            "3fbd01c00f7777772e6578616d706c652e636f6dc10c2f73616d706c652f70617468",
            106,
            {
                Field::Authority,
                String::from("www.example.com"),
                Field::Path,
                String::from("/sample/path"),
            },
        );
    }

    #[test]
    ///The encoder inserts a header into the dynamic table with a literal name.
    /// The decoder acknowledges receipt of the entry. The encoder does not send any
    /// encoded field sections.
    fn speculative_insert() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 0);
        let _ = encoder.set_capacity(220, &mut encoder_buf[..]);
        let mut encoder_cur = 0;
        qpack_test_cases!(
            encoder,
            encoder_buf,
            encoder_cur,
            3, "028010",
            "4a637573746f6d2d6b65790c637573746f6d2d76616c7565",
            54,
            {
                Field::Other(String::from("custom-key")),
                String::from("custom-value"),
            },
        );
    }

    #[test]
    fn duplicate_instruction_stream_cancellation() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
        let _ = encoder.set_capacity(4096, &mut encoder_buf[..]);
        encoder
            .table
            .update(Field::Authority, String::from("www.example.com"));
        encoder
            .table
            .update(Field::Path, String::from("/sample/path"));
        encoder.table.update(
            Field::Other(String::from("custom-key")),
            String::from("custom-value"),
        );
        encoder.required_insert_count = 3;
        let mut encoder_cur = 0;
        qpack_test_cases!(
            encoder,
            encoder_buf,
            encoder_cur,
            5, "050080c181",
            "02",
            274,
            {
                Field::Authority,
                String::from("www.example.com"),
                Field::Path,
                String::from("/"),
                Field::Other(String::from("custom-key")),
                String::from("custom-value")
            },
        );
    }

    #[test]
    fn dynamic_table_insert_eviction() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
        let _ = encoder.set_capacity(4096, &mut encoder_buf[..]);
        encoder
            .table
            .update(Field::Authority, String::from("www.example.com"));
        encoder
            .table
            .update(Field::Path, String::from("/sample/path"));
        encoder.table.update(
            Field::Other(String::from("custom-key")),
            String::from("custom-value"),
        );
        encoder
            .table
            .update(Field::Authority, String::from("www.example.com"));
        encoder.required_insert_count = 3; //acked
        let mut encoder_cur = 0;
        qpack_test_cases!(
            encoder,
            encoder_buf,
            encoder_cur,
            3, "040183",
            "810d637573746f6d2d76616c756532",
            272,
            {
                Field::Other(String::from("custom-key")),
                String::from("custom-value2")
            },
        );
    }

    #[test]
    fn test_ack() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
        let mut encoder_cur = encoder.set_capacity(4096, &mut encoder_buf[..]);

        let field_list: [(Field, String); 3] = [
            (Field::Authority, String::from("www.example.com")),
            (Field::Path, String::from("/sample/path")),
            (
                Field::Other(String::from("custom-key")),
                String::from("custom-value"),
            ),
        ];
        let mut stream_cur = 0;
        for (field, value) in field_list.iter() {
            let mut parts = Parts::new();
            parts.update(field.clone(), value.clone());
            encoder.set_parts(parts);
            let mut stream_buf = [0u8; 1024];
            let (cur1, cur2, _) = encoder.encode(
                &mut encoder_buf[encoder_cur..],
                &mut stream_buf[stream_cur..],
            );
            encoder_cur += cur1;
            stream_cur += cur2;
        }
        let _ = encoder.decode_ins(decode("80").unwrap().as_slice());
        assert_eq!(encoder.table.known_received_count, 3);
    }

    #[test]
    fn encode_post_name() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, true, 1);
        let _ = encoder.set_capacity(60, &mut encoder_buf[..]);
        let mut encoder_cur = 0;
        let mut stream_buf = [0u8; 100];
        let mut stream_cur = 0;
        let mut parts = Parts::new();
        parts.update(
            Field::Other(String::from("custom-key")),
            String::from("custom-value1"),
        );
        encoder.set_parts(parts);
        let (cur1, cur2, _) = encoder.encode(
            &mut encoder_buf[encoder_cur..],
            &mut stream_buf[stream_cur..],
        );
        encoder_cur += cur1;
        stream_cur += cur2;
        let mut parts = Parts::new();
        parts.update(
            Field::Other(String::from("custom-key")),
            String::from("custom-value2"),
        );
        encoder.set_parts(parts);
        let (cur1, cur2, _) = encoder.encode(
            &mut encoder_buf[encoder_cur..],
            &mut stream_buf[stream_cur..],
        );
        encoder_cur += cur1;
        stream_cur += cur2;
        assert_eq!(
            [16, 0, 13, 99, 117, 115, 116, 111, 109, 45, 118, 97, 108, 117, 101, 50],
            stream_buf[..stream_cur]
        );
        assert_eq!(
            [
                74, 99, 117, 115, 116, 111, 109, 45, 107, 101, 121, 13, 99, 117, 115, 116, 111,
                109, 45, 118, 97, 108, 117, 101, 49
            ],
            encoder_buf[..encoder_cur]
        )
    }
    #[test]
    fn test_indexing_with_litreal() {
        let mut encoder_buf = [0u8; 1024];
        let mut table = DynamicTable::with_empty();
        let mut encoder = QpackEncoder::new(&mut table, 0, false, 1);
        let _ = encoder.set_capacity(60, &mut encoder_buf[..]);
        let encoder_cur = 0;
        let mut stream_buf = [0u8; 100];
        let mut stream_cur = 0;
        let mut parts = Parts::new();
        parts.update(
            Field::Other(String::from("custom-key")),
            String::from("custom-value1"),
        );
        encoder.set_parts(parts);
        let (_, cur2, _) = encoder.encode(
            &mut encoder_buf[encoder_cur..],
            &mut stream_buf[stream_cur..],
        );
        stream_cur += cur2;

        assert_eq!(
            [
                39, 3, 99, 117, 115, 116, 111, 109, 45, 107, 101, 121, 13, 99, 117, 115, 116, 111,
                109, 45, 118, 97, 108, 117, 101, 49
            ],
            stream_buf[..stream_cur]
        );
    }
}
