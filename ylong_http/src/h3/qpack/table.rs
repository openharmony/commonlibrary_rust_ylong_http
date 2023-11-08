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

use std::collections::{HashMap, VecDeque};

/// The [`Dynamic Table`][dynamic_table] implementation of [QPACK].
///
/// [dynamic_table]: https://www.rfc-editor.org/rfc/rfc9204.html#name-dynamic-table
/// [QPACK]: https://www.rfc-editor.org/rfc/rfc9204.html
/// # Introduction
/// The dynamic table consists of a list of field lines maintained in first-in, first-out order.
/// A QPACK encoder and decoder share a dynamic table that is initially empty.
/// The encoder adds entries to the dynamic table and sends them to the decoder via instructions on
/// the encoder stream
///
/// The dynamic table can contain duplicate entries (i.e., entries with the same name and same value).
/// Therefore, duplicate entries MUST NOT be treated as an error by the decoder.
///
/// Dynamic table entries can have empty values.

pub(crate) struct TableSearcher<'a> {
    dynamic: &'a DynamicTable,
}

impl<'a> TableSearcher<'a> {
    pub(crate) fn new(dynamic: &'a DynamicTable) -> Self {
        Self { dynamic }
    }

    /// Searches index in static and dynamic tables.
    pub(crate) fn find_index_static(&self, header: &Field, value: &str) -> Option<TableIndex> {
        match StaticTable::index(header, value) {
            x @ Some(TableIndex::Field(_)) => x,
            _ => Some(TableIndex::None),
        }
    }

    pub(crate) fn find_index_name_static(&self, header: &Field, value: &str) -> Option<TableIndex> {
        match StaticTable::index(header, value) {
            x @ Some(TableIndex::FieldName(_)) => x,
            _ => Some(TableIndex::None),
        }
    }

    pub(crate) fn find_index_dynamic(&self, header: &Field, value: &str) -> Option<TableIndex> {
        match self.dynamic.index(header, value) {
            x @ Some(TableIndex::Field(_)) => x,
            _ => Some(TableIndex::None),
        }
    }

    pub(crate) fn find_index_name_dynamic(
        &self,
        header: &Field,
        value: &str,
    ) -> Option<TableIndex> {
        match self.dynamic.index_name(header, value) {
            x @ Some(TableIndex::FieldName(_)) => x,
            _ => Some(TableIndex::None),
        }
    }

    pub(crate) fn find_field_static(&self, index: usize) -> Option<(Field, String)> {
        match StaticTable::field(index) {
            x @ Some((_, _)) => x,
            _ => None,
        }
    }

    pub(crate) fn find_field_name_static(&self, index: usize) -> Option<Field> {
        StaticTable::field_name(index)
    }

    pub(crate) fn find_field_dynamic(&self, index: usize) -> Option<(Field, String)> {
        self.dynamic.field(index)
    }

    pub(crate) fn find_field_name_dynamic(&self, index: usize) -> Option<Field> {
        self.dynamic.field_name(index)
    }
}

pub struct DynamicTable {
    queue: VecDeque<(Field, String)>,
    // The size of the dynamic table is the sum of the size of its entries
    size: usize,
    capacity: usize,
    pub(crate) insert_count: usize,
    remove_count: usize,
    pub(crate) known_received_count: usize,
}

impl DynamicTable {
    pub fn with_empty() -> Self {
        Self {
            queue: VecDeque::new(),
            size: 0,
            capacity: 0,
            insert_count: 0,
            remove_count: 0,
            known_received_count: 0,
        }
    }

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn max_entries(&self) -> usize {
        self.capacity / 32
    }
    /// Updates `DynamicTable` by a given `Header` and value pair.
    pub(crate) fn update(&mut self, field: Field, value: String) -> Option<TableIndex> {
        self.insert_count += 1;
        self.size += field.len() + value.len() + 32;
        self.queue.push_back((field.clone(), value.clone()));
        self.fit_size();
        self.index(&field, &value)
    }

    pub(crate) fn have_enough_space(
        &self,
        field: &Field,
        value: &String,
        insert_length: &usize,
    ) -> bool {
        if self.size + field.len() + value.len() + 32 <= self.capacity - insert_length {
            return true;
        } else {
            let mut eviction_space = 0;
            for (i, (h, v)) in self.queue.iter().enumerate() {
                if i <= self.known_received_count {
                    eviction_space += h.len() + v.len() + 32;
                } else {
                    if eviction_space - insert_length >= field.len() + value.len() + 32 {
                        return true;
                    }
                    return false;
                }
                if eviction_space - insert_length >= field.len() + value.len() + 32 {
                    return true;
                }
            }
        }
        false
    }

    /// Updates `DynamicTable`'s size.
    pub(crate) fn update_size(&mut self, max_size: usize) {
        self.capacity = max_size;
        self.fit_size();
    }

    /// Adjusts dynamic table content to fit its size.
    fn fit_size(&mut self) {
        while self.size > self.capacity && !self.queue.is_empty() {
            let (key, string) = self.queue.pop_front().unwrap();
            self.remove_count += 1;
            self.capacity -= key.len() + string.len() + 32;
        }
    }

    /// Tries get the index of a `Header`.
    fn index(&self, header: &Field, value: &str) -> Option<TableIndex> {
        // find latest
        let mut index = None;
        for (n, (h, v)) in self.queue.iter().enumerate() {
            if let (true, true, _) = (header == h, value == v, &index) {
                index = Some(TableIndex::Field(n + self.remove_count))
            }
        }
        index
    }

    fn index_name(&self, header: &Field, value: &str) -> Option<TableIndex> {
        // find latest
        let mut index = None;
        for (n, (h, v)) in self.queue.iter().enumerate() {
            if let (true, _, _) = (header == h, value == v, &index) {
                index = Some(TableIndex::FieldName(n + self.remove_count))
            }
        }
        index
    }

    pub(crate) fn field(&self, index: usize) -> Option<(Field, String)> {
        self.queue.get(index - self.remove_count).cloned()
    }

    pub(crate) fn field_name(&self, index: usize) -> Option<Field> {
        self.queue
            .get(index - self.remove_count)
            .map(|(field, _)| field.clone())
    }
}

#[derive(PartialEq, Clone)]
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
///

struct StaticTable;

impl StaticTable {
    /// Gets a `Field` by the given index.
    fn field_name(index: usize) -> Option<Field> {
        match index {
            0 => Some(Field::Authority),
            1 => Some(Field::Path),
            2 => Some(Field::Other(String::from("age"))),
            3 => Some(Field::Other(String::from("content-disposition"))),
            4 => Some(Field::Other(String::from("content-length"))),
            5 => Some(Field::Other(String::from("cookie"))),
            6 => Some(Field::Other(String::from("date"))),
            7 => Some(Field::Other(String::from("etag"))),
            8 => Some(Field::Other(String::from("if-modified-since"))),
            9 => Some(Field::Other(String::from("if-none-match"))),
            10 => Some(Field::Other(String::from("last-modified"))),
            11 => Some(Field::Other(String::from("link"))),
            12 => Some(Field::Other(String::from("location"))),
            13 => Some(Field::Other(String::from("referer"))),
            14 => Some(Field::Other(String::from("set-cookie"))),
            15..=21 => Some(Field::Method),
            22..=23 => Some(Field::Scheme),
            24..=28 => Some(Field::Status),
            29..=30 => Some(Field::Other(String::from("accept"))),
            31 => Some(Field::Other(String::from("accept-encoding"))),
            32 => Some(Field::Other(String::from("accept-ranges"))),
            33..=34 => Some(Field::Other(String::from("access-control-allow-headers"))),
            35 => Some(Field::Other(String::from("access-control-allow-origin"))),
            36..=41 => Some(Field::Other(String::from("cache-control"))),
            42..=43 => Some(Field::Other(String::from("content-encoding"))),
            44..=54 => Some(Field::Other(String::from("content-type"))),
            55 => Some(Field::Other(String::from("range"))),
            56..=58 => Some(Field::Other(String::from("strict-transport-security"))),
            59..=60 => Some(Field::Other(String::from("vary"))),
            61 => Some(Field::Other(String::from("x-content-type-options"))),
            62 => Some(Field::Other(String::from("x-xss-protection"))),
            63..=71 => Some(Field::Status),
            72 => Some(Field::Other(String::from("accept-language"))),
            73..=74 => Some(Field::Other(String::from(
                "access-control-allow-credentials",
            ))),
            75 => Some(Field::Other(String::from("access-control-allow-headers"))),
            76..=78 => Some(Field::Other(String::from("access-control-allow-methods"))),
            79 => Some(Field::Other(String::from("access-control-expose-headers"))),
            80 => Some(Field::Other(String::from("access-control-request-headers"))),
            81..=82 => Some(Field::Other(String::from("access-control-request-method"))),
            83 => Some(Field::Other(String::from("alt-svc"))),
            84 => Some(Field::Other(String::from("authorization"))),
            85 => Some(Field::Other(String::from("content-security-policy"))),
            86 => Some(Field::Other(String::from("early-data"))),
            87 => Some(Field::Other(String::from("expect-ct"))),
            88 => Some(Field::Other(String::from("forwarded"))),
            89 => Some(Field::Other(String::from("if-range"))),
            90 => Some(Field::Other(String::from("origin"))),
            91 => Some(Field::Other(String::from("purpose"))),
            92 => Some(Field::Other(String::from("server"))),
            93 => Some(Field::Other(String::from("timing-allow-origin"))),
            94 => Some(Field::Other(String::from("upgrade-insecure-requests"))),
            95 => Some(Field::Other(String::from("user-agent"))),
            96 => Some(Field::Other(String::from("x-forwarded-for"))),
            97..=98 => Some(Field::Other(String::from("x-frame-options"))),
            _ => None,
        }
    }

    /// Tries to get a `Field` and a value by the given index.
    fn field(index: usize) -> Option<(Field, String)> {
        match index {
            1 => Some((Field::Path, String::from("/"))),
            2 => Some((Field::Other(String::from("age")), String::from("0"))),
            4 => Some((
                Field::Other(String::from("content-length")),
                String::from("0"),
            )),
            15 => Some((Field::Method, String::from("CONNECT"))),
            16 => Some((Field::Method, String::from("DELETE"))),
            17 => Some((Field::Method, String::from("GET"))),
            18 => Some((Field::Method, String::from("HEAD"))),
            19 => Some((Field::Method, String::from("OPTIONS"))),
            20 => Some((Field::Method, String::from("POST"))),
            21 => Some((Field::Method, String::from("PUT"))),
            22 => Some((Field::Scheme, String::from("http"))),
            23 => Some((Field::Scheme, String::from("https"))),
            24 => Some((Field::Status, String::from("103"))),
            25 => Some((Field::Status, String::from("200"))),
            26 => Some((Field::Status, String::from("304"))),
            27 => Some((Field::Status, String::from("404"))),
            28 => Some((Field::Status, String::from("503"))),
            29 => Some((Field::Other(String::from("accept")), String::from("*/*"))),
            30 => Some((
                Field::Other(String::from("accept")),
                String::from("application/dns-message"),
            )),
            31 => Some((
                Field::Other(String::from("accept-encoding")),
                String::from("gzip, deflate, br"),
            )),
            32 => Some((
                Field::Other(String::from("accept-ranges")),
                String::from("bytes"),
            )),
            33 => Some((
                Field::Other(String::from("access-control-allow-headers")),
                String::from("cache-control"),
            )),
            34 => Some((
                Field::Other(String::from("access-control-allow-headers")),
                String::from("content-type"),
            )),
            35 => Some((
                Field::Other(String::from("access-control-allow-origin")),
                String::from("*"),
            )),
            36 => Some((
                Field::Other(String::from("cache-control")),
                String::from("max-age=0"),
            )),
            37 => Some((
                Field::Other(String::from("cache-control")),
                String::from("max-age=2592000"),
            )),
            38 => Some((
                Field::Other(String::from("cache-control")),
                String::from("max-age=604800"),
            )),
            39 => Some((
                Field::Other(String::from("cache-control")),
                String::from("no-cache"),
            )),
            40 => Some((
                Field::Other(String::from("cache-control")),
                String::from("no-store"),
            )),
            41 => Some((
                Field::Other(String::from("cache-control")),
                String::from("public, max-age=31536000"),
            )),
            42 => Some((
                Field::Other(String::from("content-encoding")),
                String::from("br"),
            )),
            43 => Some((
                Field::Other(String::from("content-encoding")),
                String::from("gzip"),
            )),
            44 => Some((
                Field::Other(String::from("content-type")),
                String::from("application/dns-message"),
            )),
            45 => Some((
                Field::Other(String::from("content-type")),
                String::from("application/javascript"),
            )),
            46 => Some((
                Field::Other(String::from("content-type")),
                String::from("application/json"),
            )),
            47 => Some((
                Field::Other(String::from("content-type")),
                String::from("application/x-www-form-urlencoded"),
            )),
            48 => Some((
                Field::Other(String::from("content-type")),
                String::from("image/gif"),
            )),
            49 => Some((
                Field::Other(String::from("content-type")),
                String::from("image/jpeg"),
            )),
            50 => Some((
                Field::Other(String::from("content-type")),
                String::from("image/png"),
            )),
            51 => Some((
                Field::Other(String::from("content-type")),
                String::from("text/css"),
            )),
            52 => Some((
                Field::Other(String::from("content-type")),
                String::from("text/html; charset=utf-8"),
            )),
            53 => Some((
                Field::Other(String::from("content-type")),
                String::from("text/plain"),
            )),
            54 => Some((
                Field::Other(String::from("content-type")),
                String::from("text/plain;charset=utf-8"),
            )),
            55 => Some((
                Field::Other(String::from("range")),
                String::from("bytes=0-"),
            )),
            56 => Some((
                Field::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000"),
            )),
            57 => Some((
                Field::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000; includesubdomains"),
            )),
            58 => Some((
                Field::Other(String::from("strict-transport-security")),
                String::from("max-age=31536000; includesubdomains; preload"),
            )),
            59 => Some((
                Field::Other(String::from("vary")),
                String::from("accept-encoding"),
            )),
            60 => Some((Field::Other(String::from("vary")), String::from("origin"))),
            61 => Some((
                Field::Other(String::from("x-content-type-options")),
                String::from("nosniff"),
            )),
            62 => Some((
                Field::Other(String::from("x-xss-protection")),
                String::from("1; mode=block"),
            )),
            63 => Some((Field::Status, String::from("100"))),
            64 => Some((Field::Status, String::from("204"))),
            65 => Some((Field::Status, String::from("206"))),
            66 => Some((Field::Status, String::from("302"))),
            67 => Some((Field::Status, String::from("400"))),
            68 => Some((Field::Status, String::from("403"))),
            69 => Some((Field::Status, String::from("421"))),
            70 => Some((Field::Status, String::from("425"))),
            71 => Some((Field::Status, String::from("500"))),
            73 => Some((
                Field::Other(String::from("access-control-allow-credentials")),
                String::from("FALSE"),
            )),
            74 => Some((
                Field::Other(String::from("access-control-allow-credentials")),
                String::from("TRUE"),
            )),
            75 => Some((
                Field::Other(String::from("access-control-allow-headers")),
                String::from("*"),
            )),
            76 => Some((
                Field::Other(String::from("access-control-allow-methods")),
                String::from("get"),
            )),
            77 => Some((
                Field::Other(String::from("access-control-allow-methods")),
                String::from("get, post, options"),
            )),
            78 => Some((
                Field::Other(String::from("access-control-allow-methods")),
                String::from("options"),
            )),
            79 => Some((
                Field::Other(String::from("access-control-expose-headers")),
                String::from("content-length"),
            )),
            80 => Some((
                Field::Other(String::from("access-control-request-headers")),
                String::from("content-type"),
            )),
            81 => Some((
                Field::Other(String::from("access-control-request-method")),
                String::from("get"),
            )),
            82 => Some((
                Field::Other(String::from("access-control-request-method")),
                String::from("post"),
            )),
            83 => Some((Field::Other(String::from("alt-svc")), String::from("clear"))),
            85 => Some((
                Field::Other(String::from("content-security-policy")),
                String::from("script-src 'none'; object-src 'none'; base-uri 'none'"),
            )),
            86 => Some((Field::Other(String::from("early-data")), String::from("1"))),
            91 => Some((
                Field::Other(String::from("purpose")),
                String::from("prefetch"),
            )),
            93 => Some((
                Field::Other(String::from("timing-allow-origin")),
                String::from("*"),
            )),
            94 => Some((
                Field::Other(String::from("upgrade-insecure-requests")),
                String::from("1"),
            )),
            97 => Some((
                Field::Other(String::from("x-frame-options")),
                String::from("deny"),
            )),
            98 => Some((
                Field::Other(String::from("x-frame-options")),
                String::from("sameorigin"),
            )),
            _ => None,
        }
    }

    /// Tries to get a `Index` by the given field and value.
    fn index(field: &Field, value: &str) -> Option<TableIndex> {
        match (field, value) {
            (Field::Authority, _) => Some(TableIndex::FieldName(0)),
            (Field::Path, "/") => Some(TableIndex::Field(1)),
            (Field::Path, _) => Some(TableIndex::FieldName(1)),
            (Field::Method, "CONNECT") => Some(TableIndex::Field(15)),
            (Field::Method, "DELETE") => Some(TableIndex::Field(16)),
            (Field::Method, "GET") => Some(TableIndex::Field(17)),
            (Field::Method, "HEAD") => Some(TableIndex::Field(18)),
            (Field::Method, "OPTIONS") => Some(TableIndex::Field(19)),
            (Field::Method, "POST") => Some(TableIndex::Field(20)),
            (Field::Method, "PUT") => Some(TableIndex::Field(21)),
            (Field::Method, _) => Some(TableIndex::FieldName(15)),
            (Field::Scheme, "http") => Some(TableIndex::Field(22)),
            (Field::Scheme, "https") => Some(TableIndex::Field(23)),
            (Field::Scheme, _) => Some(TableIndex::FieldName(22)),
            (Field::Status, "103") => Some(TableIndex::Field(24)),
            (Field::Status, "200") => Some(TableIndex::Field(25)),
            (Field::Status, "304") => Some(TableIndex::Field(26)),
            (Field::Status, "404") => Some(TableIndex::Field(27)),
            (Field::Status, "503") => Some(TableIndex::Field(28)),
            (Field::Status, "100") => Some(TableIndex::Field(63)),
            (Field::Status, "204") => Some(TableIndex::Field(64)),
            (Field::Status, "206") => Some(TableIndex::Field(65)),
            (Field::Status, "302") => Some(TableIndex::Field(66)),
            (Field::Status, "400") => Some(TableIndex::Field(67)),
            (Field::Status, "403") => Some(TableIndex::Field(68)),
            (Field::Status, "421") => Some(TableIndex::Field(69)),
            (Field::Status, "425") => Some(TableIndex::Field(70)),
            (Field::Status, "500") => Some(TableIndex::Field(71)),
            (Field::Status, _) => Some(TableIndex::FieldName(24)),
            (Field::Other(s), v) => match (s.as_str(), v) {
                ("age", "0") => Some(TableIndex::Field(2)),
                ("age", _) => Some(TableIndex::FieldName(2)),
                ("content-disposition", _) => Some(TableIndex::FieldName(3)),
                ("content-length", "0") => Some(TableIndex::Field(4)),
                ("content-length", _) => Some(TableIndex::FieldName(4)),
                ("cookie", _) => Some(TableIndex::FieldName(5)),
                ("date", _) => Some(TableIndex::FieldName(6)),
                ("etag", _) => Some(TableIndex::FieldName(7)),
                ("if-modified-since", _) => Some(TableIndex::FieldName(8)),
                ("if-none-match", _) => Some(TableIndex::FieldName(9)),
                ("last-modified", _) => Some(TableIndex::FieldName(10)),
                ("link", _) => Some(TableIndex::FieldName(11)),
                ("location", _) => Some(TableIndex::FieldName(12)),
                ("referer", _) => Some(TableIndex::FieldName(13)),
                ("set-cookie", _) => Some(TableIndex::FieldName(14)),
                ("accept", "*/*") => Some(TableIndex::Field(29)),
                ("accept", "application/dns-message") => Some(TableIndex::Field(30)),
                ("accept", _) => Some(TableIndex::FieldName(29)),
                ("accept-encoding", "gzip, deflate, br") => Some(TableIndex::Field(31)),
                ("accept-encoding", _) => Some(TableIndex::FieldName(31)),
                ("accept-ranges", "bytes") => Some(TableIndex::Field(32)),
                ("accept-ranges", _) => Some(TableIndex::FieldName(32)),
                ("access-control-allow-headers", "cache-control") => Some(TableIndex::Field(33)),
                ("access-control-allow-headers", "content-type") => Some(TableIndex::Field(34)),
                ("access-control-allow-origin", "*") => Some(TableIndex::Field(35)),
                ("access-control-allow-origin", _) => Some(TableIndex::FieldName(35)),
                ("cache-control", "max-age=0") => Some(TableIndex::Field(36)),
                ("cache-control", "max-age=2592000") => Some(TableIndex::Field(37)),
                ("cache-control", "max-age=604800") => Some(TableIndex::Field(38)),
                ("cache-control", "no-cache") => Some(TableIndex::Field(39)),
                ("cache-control", "no-store") => Some(TableIndex::Field(40)),
                ("cache-control", "public, max-age=31536000") => Some(TableIndex::Field(41)),
                ("cache-control", _) => Some(TableIndex::FieldName(36)),
                ("content-encoding", "br") => Some(TableIndex::Field(42)),
                ("content-encoding", "gzip") => Some(TableIndex::Field(43)),
                ("content-encoding", _) => Some(TableIndex::FieldName(42)),
                ("content-type", "application/dns-message") => Some(TableIndex::Field(44)),
                ("content-type", "application/javascript") => Some(TableIndex::Field(45)),
                ("content-type", "application/json") => Some(TableIndex::Field(46)),
                ("content-type", "application/x-www-form-urlencoded") => {
                    Some(TableIndex::Field(47))
                }
                ("content-type", "image/gif") => Some(TableIndex::Field(48)),
                ("content-type", "image/jpeg") => Some(TableIndex::Field(49)),
                ("content-type", "image/png") => Some(TableIndex::Field(50)),
                ("content-type", "text/css") => Some(TableIndex::Field(51)),
                ("content-type", "text/html; charset=utf-8") => Some(TableIndex::Field(52)),
                ("content-type", "text/plain") => Some(TableIndex::Field(53)),
                ("content-type", "text/plain;charset=utf-8") => Some(TableIndex::Field(54)),
                ("content-type", _) => Some(TableIndex::FieldName(44)),
                ("range", "bytes=0-") => Some(TableIndex::Field(55)),
                ("range", _) => Some(TableIndex::FieldName(55)),
                ("strict-transport-security", "max-age=31536000") => Some(TableIndex::Field(56)),
                ("strict-transport-security", "max-age=31536000; includesubdomains") => {
                    Some(TableIndex::Field(57))
                }
                ("strict-transport-security", "max-age=31536000; includesubdomains; preload") => {
                    Some(TableIndex::Field(58))
                }
                ("strict-transport-security", _) => Some(TableIndex::FieldName(56)),
                ("vary", "accept-encoding") => Some(TableIndex::Field(59)),
                ("vary", "origin") => Some(TableIndex::Field(60)),
                ("vary", _) => Some(TableIndex::FieldName(59)),
                ("x-content-type-options", "nosniff") => Some(TableIndex::Field(61)),
                ("x-content-type-options", _) => Some(TableIndex::FieldName(61)),
                ("x-xss-protection", "1; mode=block") => Some(TableIndex::Field(62)),
                ("x-xss-protection", _) => Some(TableIndex::FieldName(62)),
                ("accept-language", _) => Some(TableIndex::FieldName(72)),
                ("access-control-allow-credentials", "FALSE") => Some(TableIndex::Field(73)),
                ("access-control-allow-credentials", "TRUE") => Some(TableIndex::Field(74)),
                ("access-control-allow-credentials", _) => Some(TableIndex::FieldName(73)),
                ("access-control-allow-headers", "*") => Some(TableIndex::Field(75)),
                ("access-control-allow-headers", _) => Some(TableIndex::FieldName(75)),
                ("access-control-allow-methods", "get") => Some(TableIndex::Field(76)),
                ("access-control-allow-methods", "get, post, options") => {
                    Some(TableIndex::Field(77))
                }
                ("access-control-allow-methods", "options") => Some(TableIndex::Field(78)),
                ("access-control-allow-methods", _) => Some(TableIndex::FieldName(76)),
                ("access-control-expose-headers", "content-length") => Some(TableIndex::Field(79)),
                ("access-control-expose-headers", _) => Some(TableIndex::FieldName(79)),
                ("access-control-request-headers", "content-type") => Some(TableIndex::Field(80)),
                ("access-control-request-headers", _) => Some(TableIndex::FieldName(80)),
                ("access-control-request-method", "get") => Some(TableIndex::Field(81)),
                ("access-control-request-method", "post") => Some(TableIndex::Field(82)),
                ("access-control-request-method", _) => Some(TableIndex::FieldName(81)),
                ("alt-svc", "clear") => Some(TableIndex::Field(83)),
                ("alt-svc", _) => Some(TableIndex::FieldName(83)),
                ("authorization", _) => Some(TableIndex::FieldName(84)),
                (
                    "content-security-policy",
                    "script-src 'none'; object-src 'none'; base-uri 'none'",
                ) => Some(TableIndex::Field(85)),
                ("content-security-policy", _) => Some(TableIndex::FieldName(85)),
                ("early-data", "1") => Some(TableIndex::Field(86)),
                ("early-data", _) => Some(TableIndex::FieldName(86)),
                ("expect-ct", _) => Some(TableIndex::FieldName(87)),
                ("forwarded", _) => Some(TableIndex::FieldName(88)),
                ("if-range", _) => Some(TableIndex::FieldName(89)),
                ("origin", _) => Some(TableIndex::FieldName(90)),
                ("purpose", "prefetch") => Some(TableIndex::Field(91)),
                ("purpose", _) => Some(TableIndex::FieldName(91)),
                ("server", _) => Some(TableIndex::FieldName(92)),
                ("timing-allow-origin", "*") => Some(TableIndex::Field(93)),
                ("timing-allow-origin", _) => Some(TableIndex::FieldName(93)),
                ("upgrade-insecure-requests", "1") => Some(TableIndex::Field(94)),
                ("upgrade-insecure-requests", _) => Some(TableIndex::FieldName(94)),
                ("user-agent", _) => Some(TableIndex::FieldName(95)),
                ("x-forwarded-for", _) => Some(TableIndex::FieldName(96)),
                ("x-frame-options", "deny") => Some(TableIndex::Field(97)),
                ("x-frame-options", "sameorigin") => Some(TableIndex::Field(98)),
                ("x-frame-options", _) => Some(TableIndex::FieldName(97)),
                _ => None,
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Field {
    Authority,
    Method,
    Path,
    Scheme,
    Status,
    Other(String),
}

impl Field {
    pub(crate) fn len(&self) -> usize {
        match self {
            Field::Authority => 10, // 10 is the length of ":authority".
            Field::Method => 7,     // 7 is the length of ":method".
            Field::Path => 5,       // 5 is the length of ":path".
            Field::Scheme => 7,     // 7 is the length of "scheme".
            Field::Status => 7,     // 7 is the length of "status".
            Field::Other(s) => s.len(),
        }
    }

    pub(crate) fn into_string(self) -> String {
        match self {
            Field::Authority => String::from(":authority"),
            Field::Method => String::from(":method"),
            Field::Path => String::from(":path"),
            Field::Scheme => String::from(":scheme"),
            Field::Status => String::from(":status"),
            Field::Other(s) => s,
        }
    }
}
