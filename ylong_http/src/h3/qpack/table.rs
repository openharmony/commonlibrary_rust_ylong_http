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

use std::collections::{HashMap, VecDeque};

use crate::h3::qpack::error::QpackError;

/// The [`Dynamic Table`][dynamic_table] implementation of [QPACK].
///
/// [dynamic_table]: https://www.rfc-editor.org/rfc/rfc9204.html#name-dynamic-table
/// [QPACK]: https://www.rfc-editor.org/rfc/rfc9204.html
/// # Introduction
/// The dynamic table consists of a list of field lines maintained in first-in,
/// first-out order. A QPACK encoder and decoder share a dynamic table that is
/// initially empty. The encoder adds entries to the dynamic table and sends
/// them to the decoder via instructions on the encoder stream
///
/// The dynamic table can contain duplicate entries (i.e., entries with the same
/// name and same value). Therefore, duplicate entries MUST NOT be treated as an
/// error by the decoder.
///
/// Dynamic table entries can have empty values.

pub(crate) struct TableSearcher<'a> {
    dynamic: &'a DynamicTable,
}

#[derive(Eq, PartialEq, Copy, Clone)]
pub(crate) enum SearchResult {
    StaticIndex(usize),
    StaticNameIndex(usize),
    DynamicIndex(usize),
    DynamicNameIndex(usize),
    NotFound,
}

impl<'a> TableSearcher<'a> {
    pub(crate) fn new(dynamic: &'a DynamicTable) -> Self {
        Self { dynamic }
    }

    pub(crate) fn search_in_static(&self, header: &NameField, value: &str) -> TableIndex {
        StaticTable::index(header, value)
    }

    pub(crate) fn search_in_dynamic(
        &self,
        header: &NameField,
        value: &str,
        allow_block: bool,
    ) -> TableIndex {
        self.dynamic.index(header, value, allow_block)
    }

    pub(crate) fn find_field_static(&self, index: usize) -> Option<(NameField, String)> {
        match StaticTable::field(index) {
            x @ Some((_, _)) => x,
            _ => None,
        }
    }

    pub(crate) fn find_field_name_static(&self, index: usize) -> Option<NameField> {
        StaticTable::field_name(index)
    }

    pub(crate) fn find_field_dynamic(&self, index: usize) -> Option<(NameField, String)> {
        self.dynamic.field(index)
    }

    pub(crate) fn find_field_name_dynamic(&self, index: usize) -> Option<NameField> {
        self.dynamic.field_name(index)
    }
}

#[derive(Clone, Eq, PartialEq)]
struct DynamicField {
    index: u64,
    name: NameField,
    value: String,
    tracked: usize,
}

impl DynamicField {
    pub(crate) fn index(&self) -> u64 {
        self.index
    }

    pub(crate) fn name(&self) -> &NameField {
        &self.name
    }

    pub(crate) fn value(&self) -> &str {
        self.value.as_str()
    }

    pub(crate) fn size(&self) -> usize {
        self.name.len() + self.value.len() + 32
    }

    pub(crate) fn is_tracked(&self) -> bool {
        self.tracked > 0
    }
}

pub struct DynamicTable {
    queue: VecDeque<DynamicField>,
    // The used_cap of the dynamic table is the sum of the used_cap of its entries
    used_cap: usize,
    capacity: usize,
    insert_count: usize,
    remove_count: usize,
    known_received_count: u64,
}

impl DynamicTable {
    pub fn with_empty() -> Self {
        Self {
            queue: VecDeque::new(),
            used_cap: 0,
            capacity: 0,
            insert_count: 0,
            remove_count: 0,
            known_received_count: 0,
        }
    }

    pub(crate) fn update_capacity(&mut self, new_cap: usize) -> Option<usize> {
        let mut updated = None;
        if new_cap < self.capacity {
            let required = self.capacity - new_cap;
            if let Some(size) = self.can_evict(required) {
                self.evict_drained(size);
                self.capacity = new_cap;
                updated = Some(new_cap);
            }
        } else {
            self.capacity = new_cap;
            updated = Some(new_cap);
        }
        updated
    }

    pub(crate) fn insert_count(&self) -> usize {
        self.insert_count
    }

    pub(crate) fn track_field(&mut self, index: usize) {
        if let Some(field) = self.queue.get_mut(index - self.remove_count) {
            field.tracked += 1;
        } else {
            unreachable!()
        }
    }

    pub(crate) fn untracked_field(&mut self, index: usize) {
        if let Some(field) = self.queue.get_mut(index - self.remove_count) {
            assert!(field.tracked > 0);
            field.tracked -= 1;
        } else {
            unreachable!()
        }
    }

    pub(crate) fn increase_known_receive_count(&mut self, increment: usize) {
        self.known_received_count += (increment - 1) as u64;
        // TODO 替换成error
        assert!(self.known_received_count < (self.insert_count as u64))
    }

    pub(crate) fn size(&self) -> usize {
        self.used_cap
    }

    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn can_evict(&mut self, required: usize) -> Option<usize> {
        if required > self.capacity {
            return None;
        }
        let bound = self.capacity - required;
        let mut can_evict = 0;
        let mut used_cap = self.used_cap;
        while !self.queue.is_empty() && used_cap > bound {
            if let Some(to_evict) = self.queue.front() {
                if to_evict.is_tracked() || to_evict.index() > self.known_recved_count() {
                    return None;
                }
                used_cap -= to_evict.size();
                can_evict += 1;
            }
        }
        Some(can_evict)
    }

    // Note ensure that there are enough entries in the queue before the expulsion.
    pub(crate) fn evict_drained(&mut self, size: usize) {
        let mut to_evict = size;
        while to_evict > 0 {
            if let Some(field) = self.queue.pop_front() {
                self.used_cap -= field.size();
                self.remove_count += 1;
            } else {
                unreachable!()
            }
            to_evict -= 1;
        }
    }

    pub(crate) fn known_recved_count(&self) -> u64 {
        self.known_received_count
    }

    pub(crate) fn max_entries(&self) -> usize {
        self.capacity / 32
    }

    /// Updates `DynamicTable` by a given `Header` and value pair.
    pub(crate) fn update(&mut self, field: NameField, value: String) -> usize {
        let field = DynamicField {
            index: self.insert_count() as u64,
            name: field,
            value,
            tracked: 0,
        };
        let size = field.size();
        let index = field.index();
        self.queue.push_back(field);
        self.insert_count += 1;
        self.used_cap += size;
        index as usize
    }

    /// Tries to get the index of a `Header`.
    fn index(&self, header: &NameField, value: &str, allow_block: bool) -> TableIndex {
        let mut index = TableIndex::None;

        for field in self.queue.iter() {
            // 从queue的头开始迭代，index从小到大，找到最新(大)的index
            if field.index() > self.known_recved_count() && !allow_block {
                break;
            }
            if header == field.name() {
                // find latest then return
                index = if value == field.value() {
                    TableIndex::Field(field.index() as usize)
                } else {
                    TableIndex::FieldName(field.index() as usize)
                }
            }
        }
        index
    }

    pub(crate) fn field(&self, index: usize) -> Option<(NameField, String)> {
        self.queue
            .get(index - self.remove_count)
            .cloned()
            .map(|field| (field.name, field.value))
    }

    pub(crate) fn field_name(&self, index: usize) -> Option<NameField> {
        self.queue
            .get(index - self.remove_count)
            .map(|field| field.name().clone())
    }

    /// Updates `DynamicTable`'s size.
    pub(crate) fn update_size(&mut self, max_size: usize) {
        self.capacity = max_size;
        self.fit_size();
    }

    /// Adjusts dynamic table content to fit its size.
    fn fit_size(&mut self) {
        while self.used_cap > self.capacity && !self.queue.is_empty() {
            let field = self.queue.pop_front().unwrap();
            self.remove_count += 1;
            self.used_cap -= field.size();
        }
    }
}

#[derive(PartialEq, Copy, Clone)]
pub(crate) enum TableIndex {
    Field(usize),
    FieldName(usize),
    None,
}

/// The [`Static Table`][static_table] implementation of [QPACK].
///
/// [static_table]: https://www.rfc-editor.org/rfc/rfc9204.html#static-table
/// [QPACK]: https://www.rfc-editor.org/rfc/rfc9204.html
///
/// # Introduction
/// The static table consists of a predefined list of field lines,
/// each of which has a fixed index over time.
/// All entries in the static table have a name and a value.
/// However, values can be empty (that is, have a length of 0). Each entry is
/// identified by a unique index.
/// Note that the QPACK static table is indexed from 0,
/// whereas the HPACK static table is indexed from 1.
/// When the decoder encounters an invalid static table
/// index in a field line format, it MUST treat this
/// as a connection error of type QpackDecompressionFailed.
/// If this index is received on the encoder stream,
/// this MUST be treated as a connection error of type QpackEncoderStreamError.

struct StaticTable;

impl StaticTable {
    /// Gets a `Field` by the given index.
    fn field_name(index: usize) -> Option<NameField> {
        match index {
            0 => Some(NameField::Authority),
            1 => Some(NameField::Path),
            2 => Some(NameField::Other(String::from("age"))),
            3 => Some(NameField::Other(String::from("content-disposition"))),
            4 => Some(NameField::Other(String::from("content-length"))),
            5 => Some(NameField::Other(String::from("cookie"))),
            6 => Some(NameField::Other(String::from("date"))),
            7 => Some(NameField::Other(String::from("etag"))),
            8 => Some(NameField::Other(String::from("if-modified-since"))),
            9 => Some(NameField::Other(String::from("if-none-match"))),
            10 => Some(NameField::Other(String::from("last-modified"))),
            11 => Some(NameField::Other(String::from("link"))),
            12 => Some(NameField::Other(String::from("location"))),
            13 => Some(NameField::Other(String::from("referer"))),
            14 => Some(NameField::Other(String::from("set-cookie"))),
            15..=21 => Some(NameField::Method),
            22..=23 => Some(NameField::Scheme),
            24..=28 => Some(NameField::Status),
            29..=30 => Some(NameField::Other(String::from("accept"))),
            31 => Some(NameField::Other(String::from("accept-encoding"))),
            32 => Some(NameField::Other(String::from("accept-ranges"))),
            33..=34 => Some(NameField::Other(String::from(
                "access-control-allow-headers",
            ))),
            35 => Some(NameField::Other(String::from(
                "access-control-allow-origin",
            ))),
            36..=41 => Some(NameField::Other(String::from("cache-control"))),
            42..=43 => Some(NameField::Other(String::from("content-encoding"))),
            44..=54 => Some(NameField::Other(String::from("content-type"))),
            55 => Some(NameField::Other(String::from("range"))),
            56..=58 => Some(NameField::Other(String::from("strict-transport-security"))),
            59..=60 => Some(NameField::Other(String::from("vary"))),
            61 => Some(NameField::Other(String::from("x-content-type-options"))),
            62 => Some(NameField::Other(String::from("x-xss-protection"))),
            63..=71 => Some(NameField::Status),
            72 => Some(NameField::Other(String::from("accept-language"))),
            73..=74 => Some(NameField::Other(String::from(
                "access-control-allow-credentials",
            ))),
            75 => Some(NameField::Other(String::from(
                "access-control-allow-headers",
            ))),
            76..=78 => Some(NameField::Other(String::from(
                "access-control-allow-methods",
            ))),
            79 => Some(NameField::Other(String::from(
                "access-control-expose-headers",
            ))),
            80 => Some(NameField::Other(String::from(
                "access-control-request-headers",
            ))),
            81..=82 => Some(NameField::Other(String::from(
                "access-control-request-method",
            ))),
            83 => Some(NameField::Other(String::from("alt-svc"))),
            84 => Some(NameField::Other(String::from("authorization"))),
            85 => Some(NameField::Other(String::from("content-security-policy"))),
            86 => Some(NameField::Other(String::from("early-data"))),
            87 => Some(NameField::Other(String::from("expect-ct"))),
            88 => Some(NameField::Other(String::from("forwarded"))),
            89 => Some(NameField::Other(String::from("if-range"))),
            90 => Some(NameField::Other(String::from("origin"))),
            91 => Some(NameField::Other(String::from("purpose"))),
            92 => Some(NameField::Other(String::from("server"))),
            93 => Some(NameField::Other(String::from("timing-allow-origin"))),
            94 => Some(NameField::Other(String::from("upgrade-insecure-requests"))),
            95 => Some(NameField::Other(String::from("user-agent"))),
            96 => Some(NameField::Other(String::from("x-forwarded-for"))),
            97..=98 => Some(NameField::Other(String::from("x-frame-options"))),
            _ => None,
        }
    }

    /// Tries to get a `Field` and a value by the given index.
    fn field(index: usize) -> Option<(NameField, String)> {
        match index {
            1 => Some((NameField::Path, String::from("/"))),
            2 => Some((NameField::Other(String::from("age")), String::from("0"))),
            4 => Some((
                NameField::Other(String::from("content-length")),
                String::from("0"),
            )),
            15 => Some((NameField::Method, String::from("CONNECT"))),
            16 => Some((NameField::Method, String::from("DELETE"))),
            17 => Some((NameField::Method, String::from("GET"))),
            18 => Some((NameField::Method, String::from("HEAD"))),
            19 => Some((NameField::Method, String::from("OPTIONS"))),
            20 => Some((NameField::Method, String::from("POST"))),
            21 => Some((NameField::Method, String::from("PUT"))),
            22 => Some((NameField::Scheme, String::from("http"))),
            23 => Some((NameField::Scheme, String::from("https"))),
            24 => Some((NameField::Status, String::from("103"))),
            25 => Some((NameField::Status, String::from("200"))),
            26 => Some((NameField::Status, String::from("304"))),
            27 => Some((NameField::Status, String::from("404"))),
            28 => Some((NameField::Status, String::from("503"))),
            29 => Some((
                NameField::Other(String::from("accept")),
                String::from("*/*"),
            )),
            30 => Some((
                NameField::Other(String::from("accept")),
                String::from("application/dns-message"),
            )),
            31 => Some((
                NameField::Other(String::from("accept-encoding")),
                String::from("gzip, deflate, br"),
            )),
            32 => Some((
                NameField::Other(String::from("accept-ranges")),
                String::from("bytes"),
            )),
            33 => Some((
                NameField::Other(String::from("access-control-allow-headers")),
                String::from("cache-control"),
            )),
            34 => Some((
                NameField::Other(String::from("access-control-allow-headers")),
                String::from("content-type"),
            )),
            35 => Some((
                NameField::Other(String::from("access-control-allow-origin")),
                String::from("*"),
            )),
            36 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("max-age=0"),
            )),
            37 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("max-age=2592000"),
            )),
            38 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("max-age=604800"),
            )),
            39 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("no-cache"),
            )),
            40 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("no-store"),
            )),
            41 => Some((
                NameField::Other(String::from("cache-control")),
                String::from("public, max-age=31536000"),
            )),
            42 => Some((
                NameField::Other(String::from("content-encoding")),
                String::from("br"),
            )),
            43 => Some((
                NameField::Other(String::from("content-encoding")),
                String::from("gzip"),
            )),
            44 => Some((
                NameField::Other(String::from("content-type")),
                String::from("application/dns-message"),
            )),
            45 => Some((
                NameField::Other(String::from("content-type")),
                String::from("application/javascript"),
            )),
            46 => Some((
                NameField::Other(String::from("content-type")),
                String::from("application/json"),
            )),
            47 => Some((
                NameField::Other(String::from("content-type")),
                String::from("application/x-www-form-urlencoded"),
            )),
            48 => Some((
                NameField::Other(String::from("content-type")),
                String::from("image/gif"),
            )),
            49 => Some((
                NameField::Other(String::from("content-type")),
                String::from("image/jpeg"),
            )),
            50 => Some((
                NameField::Other(String::from("content-type")),
                String::from("image/png"),
            )),
            51 => Some((
                NameField::Other(String::from("content-type")),
                String::from("text/css"),
            )),
            52 => Some((
                NameField::Other(String::from("content-type")),
                String::from("text/html; charset=utf-8"),
            )),
            53 => Some((
                NameField::Other(String::from("content-type")),
                String::from("text/plain"),
            )),
            54 => Some((
                NameField::Other(String::from("content-type")),
                String::from("text/plain;charset=utf-8"),
            )),
            55 => Some((
                NameField::Other(String::from("range")),
                String::from("bytes=0-"),
            )),
            56 => Some((
                NameField::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000"),
            )),
            57 => Some((
                NameField::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000; includesubdomains"),
            )),
            58 => Some((
                NameField::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000; includesubdomains; preload"),
            )),
            59 => Some((
                NameField::Other(String::from("vary")),
                String::from("accept-encoding"),
            )),
            60 => Some((
                NameField::Other(String::from("vary")),
                String::from("origin"),
            )),
            61 => Some((
                NameField::Other(String::from("x-content-type-options")),
                String::from("nosniff"),
            )),
            62 => Some((
                NameField::Other(String::from("x-xss-protection")),
                String::from("1; mode=block"),
            )),
            63 => Some((NameField::Status, String::from("100"))),
            64 => Some((NameField::Status, String::from("204"))),
            65 => Some((NameField::Status, String::from("206"))),
            66 => Some((NameField::Status, String::from("302"))),
            67 => Some((NameField::Status, String::from("400"))),
            68 => Some((NameField::Status, String::from("403"))),
            69 => Some((NameField::Status, String::from("421"))),
            70 => Some((NameField::Status, String::from("425"))),
            71 => Some((NameField::Status, String::from("500"))),
            73 => Some((
                NameField::Other(String::from("access-control-allow-credentials")),
                String::from("FALSE"),
            )),
            74 => Some((
                NameField::Other(String::from("access-control-allow-credentials")),
                String::from("TRUE"),
            )),
            75 => Some((
                NameField::Other(String::from("access-control-allow-headers")),
                String::from("*"),
            )),
            76 => Some((
                NameField::Other(String::from("access-control-allow-methods")),
                String::from("get"),
            )),
            77 => Some((
                NameField::Other(String::from("access-control-allow-methods")),
                String::from("get, post, options"),
            )),
            78 => Some((
                NameField::Other(String::from("access-control-allow-methods")),
                String::from("options"),
            )),
            79 => Some((
                NameField::Other(String::from("access-control-expose-headers")),
                String::from("content-length"),
            )),
            80 => Some((
                NameField::Other(String::from("access-control-request-headers")),
                String::from("content-type"),
            )),
            81 => Some((
                NameField::Other(String::from("access-control-request-method")),
                String::from("get"),
            )),
            82 => Some((
                NameField::Other(String::from("access-control-request-method")),
                String::from("post"),
            )),
            83 => Some((
                NameField::Other(String::from("alt-svc")),
                String::from("clear"),
            )),
            85 => Some((
                NameField::Other(String::from("content-security-policy")),
                String::from("script-src 'none'; object-src 'none'; base-uri 'none'"),
            )),
            86 => Some((
                NameField::Other(String::from("early-data")),
                String::from("1"),
            )),
            91 => Some((
                NameField::Other(String::from("purpose")),
                String::from("prefetch"),
            )),
            93 => Some((
                NameField::Other(String::from("timing-allow-origin")),
                String::from("*"),
            )),
            94 => Some((
                NameField::Other(String::from("upgrade-insecure-requests")),
                String::from("1"),
            )),
            97 => Some((
                NameField::Other(String::from("x-frame-options")),
                String::from("deny"),
            )),
            98 => Some((
                NameField::Other(String::from("x-frame-options")),
                String::from("sameorigin"),
            )),
            _ => None,
        }
    }

    fn index(field: &NameField, value: &str) -> TableIndex {
        match (field, value) {
            (NameField::Authority, _) => TableIndex::FieldName(0),
            (NameField::Path, "/") => TableIndex::Field(1),
            (NameField::Path, _) => TableIndex::FieldName(1),
            (NameField::Method, "CONNECT") => TableIndex::Field(15),
            (NameField::Method, "DELETE") => TableIndex::Field(16),
            (NameField::Method, "GET") => TableIndex::Field(17),
            (NameField::Method, "HEAD") => TableIndex::Field(18),
            (NameField::Method, "OPTIONS") => TableIndex::Field(19),
            (NameField::Method, "POST") => TableIndex::Field(20),
            (NameField::Method, "PUT") => TableIndex::Field(21),
            (NameField::Method, _) => TableIndex::FieldName(15),
            (NameField::Scheme, "http") => TableIndex::Field(22),
            (NameField::Scheme, "https") => TableIndex::Field(23),
            (NameField::Scheme, _) => TableIndex::FieldName(22),
            (NameField::Status, "103") => TableIndex::Field(24),
            (NameField::Status, "200") => TableIndex::Field(25),
            (NameField::Status, "304") => TableIndex::Field(26),
            (NameField::Status, "404") => TableIndex::Field(27),
            (NameField::Status, "503") => TableIndex::Field(28),
            (NameField::Status, "100") => TableIndex::Field(63),
            (NameField::Status, "204") => TableIndex::Field(64),
            (NameField::Status, "206") => TableIndex::Field(65),
            (NameField::Status, "302") => TableIndex::Field(66),
            (NameField::Status, "400") => TableIndex::Field(67),
            (NameField::Status, "403") => TableIndex::Field(68),
            (NameField::Status, "421") => TableIndex::Field(69),
            (NameField::Status, "425") => TableIndex::Field(70),
            (NameField::Status, "500") => TableIndex::Field(71),
            (NameField::Status, _) => TableIndex::FieldName(24),
            (NameField::Other(s), v) => match (s.as_str(), v) {
                ("age", "0") => TableIndex::Field(2),
                ("age", _) => TableIndex::FieldName(2),
                ("content-disposition", _) => TableIndex::FieldName(3),
                ("content-length", "0") => TableIndex::Field(4),
                ("content-length", _) => TableIndex::FieldName(4),
                ("cookie", _) => TableIndex::FieldName(5),
                ("date", _) => TableIndex::FieldName(6),
                ("etag", _) => TableIndex::FieldName(7),
                ("if-modified-since", _) => TableIndex::FieldName(8),
                ("if-none-match", _) => TableIndex::FieldName(9),
                ("last-modified", _) => TableIndex::FieldName(10),
                ("link", _) => TableIndex::FieldName(11),
                ("location", _) => TableIndex::FieldName(12),
                ("referer", _) => TableIndex::FieldName(13),
                ("set-cookie", _) => TableIndex::FieldName(14),
                ("accept", "*/*") => TableIndex::Field(29),
                ("accept", "application/dns-message") => TableIndex::Field(30),
                ("accept", _) => TableIndex::FieldName(29),
                ("accept-encoding", "gzip, deflate, br") => TableIndex::Field(31),
                ("accept-encoding", _) => TableIndex::FieldName(31),
                ("accept-ranges", "bytes") => TableIndex::Field(32),
                ("accept-ranges", _) => TableIndex::FieldName(32),
                ("access-control-allow-headers", "cache-control") => TableIndex::Field(33),
                ("access-control-allow-headers", "content-type") => TableIndex::Field(34),
                ("access-control-allow-origin", "*") => TableIndex::Field(35),
                ("access-control-allow-origin", _) => TableIndex::FieldName(35),
                ("cache-control", "max-age=0") => TableIndex::Field(36),
                ("cache-control", "max-age=2592000") => TableIndex::Field(37),
                ("cache-control", "max-age=604800") => TableIndex::Field(38),
                ("cache-control", "no-cache") => TableIndex::Field(39),
                ("cache-control", "no-store") => TableIndex::Field(40),
                ("cache-control", "public, max-age=31536000") => TableIndex::Field(41),
                ("cache-control", _) => TableIndex::FieldName(36),
                ("content-encoding", "br") => TableIndex::Field(42),
                ("content-encoding", "gzip") => TableIndex::Field(43),
                ("content-encoding", _) => TableIndex::FieldName(42),
                ("content-type", "application/dns-message") => TableIndex::Field(44),
                ("content-type", "application/javascript") => TableIndex::Field(45),
                ("content-type", "application/json") => TableIndex::Field(46),
                ("content-type", "application/x-www-form-urlencoded") => TableIndex::Field(47),
                ("content-type", "image/gif") => TableIndex::Field(48),
                ("content-type", "image/jpeg") => TableIndex::Field(49),
                ("content-type", "image/png") => TableIndex::Field(50),
                ("content-type", "text/css") => TableIndex::Field(51),
                ("content-type", "text/html; charset=utf-8") => TableIndex::Field(52),
                ("content-type", "text/plain") => TableIndex::Field(53),
                ("content-type", "text/plain;charset=utf-8") => TableIndex::Field(54),
                ("content-type", _) => TableIndex::FieldName(44),
                ("range", "bytes=0-") => TableIndex::Field(55),
                ("range", _) => TableIndex::FieldName(55),
                ("strict-transport-security", "max-age=31536000") => TableIndex::Field(56),
                ("strict-transport-security", "max-age=31536000; includesubdomains") => {
                    TableIndex::Field(57)
                }
                ("strict-transport-security", "max-age=31536000; includesubdomains; preload") => {
                    TableIndex::Field(58)
                }
                ("strict-transport-security", _) => TableIndex::FieldName(56),
                ("vary", "accept-encoding") => TableIndex::Field(59),
                ("vary", "origin") => TableIndex::Field(60),
                ("vary", _) => TableIndex::FieldName(59),
                ("x-content-type-options", "nosniff") => TableIndex::Field(61),
                ("x-content-type-options", _) => TableIndex::FieldName(61),
                ("x-xss-protection", "1; mode=block") => TableIndex::Field(62),
                ("x-xss-protection", _) => TableIndex::FieldName(62),
                ("accept-language", _) => TableIndex::FieldName(72),
                ("access-control-allow-credentials", "FALSE") => TableIndex::Field(73),
                ("access-control-allow-credentials", "TRUE") => TableIndex::Field(74),
                ("access-control-allow-credentials", _) => TableIndex::FieldName(73),
                ("access-control-allow-headers", "*") => TableIndex::Field(75),
                ("access-control-allow-headers", _) => TableIndex::FieldName(75),
                ("access-control-allow-methods", "get") => TableIndex::Field(76),
                ("access-control-allow-methods", "get, post, options") => TableIndex::Field(77),
                ("access-control-allow-methods", "options") => TableIndex::Field(78),
                ("access-control-allow-methods", _) => TableIndex::FieldName(76),
                ("access-control-expose-headers", "content-length") => TableIndex::Field(79),
                ("access-control-expose-headers", _) => TableIndex::FieldName(79),
                ("access-control-request-headers", "content-type") => TableIndex::Field(80),
                ("access-control-request-headers", _) => TableIndex::FieldName(80),
                ("access-control-request-method", "get") => TableIndex::Field(81),
                ("access-control-request-method", "post") => TableIndex::Field(82),
                ("access-control-request-method", _) => TableIndex::FieldName(81),
                ("alt-svc", "clear") => TableIndex::Field(83),
                ("alt-svc", _) => TableIndex::FieldName(83),
                ("authorization", _) => TableIndex::FieldName(84),
                (
                    "content-security-policy",
                    "script-src 'none'; object-src 'none'; base-uri 'none'",
                ) => TableIndex::Field(85),
                ("content-security-policy", _) => TableIndex::FieldName(85),
                ("early-data", "1") => TableIndex::Field(86),
                ("early-data", _) => TableIndex::FieldName(86),
                ("expect-ct", _) => TableIndex::FieldName(87),
                ("forwarded", _) => TableIndex::FieldName(88),
                ("if-range", _) => TableIndex::FieldName(89),
                ("origin", _) => TableIndex::FieldName(90),
                ("purpose", "prefetch") => TableIndex::Field(91),
                ("purpose", _) => TableIndex::FieldName(91),
                ("server", _) => TableIndex::FieldName(92),
                ("timing-allow-origin", "*") => TableIndex::Field(93),
                ("timing-allow-origin", _) => TableIndex::FieldName(93),
                ("upgrade-insecure-requests", "1") => TableIndex::Field(94),
                ("upgrade-insecure-requests", _) => TableIndex::FieldName(94),
                ("user-agent", _) => TableIndex::FieldName(95),
                ("x-forwarded-for", _) => TableIndex::FieldName(96),
                ("x-frame-options", "deny") => TableIndex::Field(97),
                ("x-frame-options", "sameorigin") => TableIndex::Field(98),
                ("x-frame-options", _) => TableIndex::FieldName(97),
                _ => TableIndex::None,
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NameField {
    Authority,
    Method,
    Path,
    Scheme,
    Status,
    Other(String),
}

impl NameField {
    pub(crate) fn len(&self) -> usize {
        match self {
            NameField::Authority => 10, // 10 is the length of ":authority".
            NameField::Method => 7,     // 7 is the length of ":method".
            NameField::Path => 5,       // 5 is the length of ":path".
            NameField::Scheme => 7,     // 7 is the length of "scheme".
            NameField::Status => 7,     // 7 is the length of "status".
            NameField::Other(s) => s.len(),
        }
    }
}

impl ToString for NameField {
    fn to_string(&self) -> String {
        match self {
            NameField::Authority => String::from(":authority"),
            NameField::Method => String::from(":method"),
            NameField::Path => String::from(":path"),
            NameField::Scheme => String::from(":scheme"),
            NameField::Status => String::from(":status"),
            NameField::Other(s) => s.clone(),
        }
    }
}
