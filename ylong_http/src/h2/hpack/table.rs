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

use std::collections::VecDeque;
use std::ops::Add;

/// `TableSearcher` is used to find specified content in static and dynamic
/// tables.
pub(crate) struct TableSearcher<'a> {
    dynamic: &'a DynamicTable,
}

impl<'a> TableSearcher<'a> {
    pub(crate) fn new(dynamic: &'a DynamicTable) -> Self {
        Self { dynamic }
    }

    /// Searches `HeaderName` in static and dynamic tables.
    pub(crate) fn search_header_name(&self, index: usize) -> Option<Header> {
        if index <= 61 {
            StaticTable::header_name(index)
        } else {
            self.dynamic.header_name(index - 62)
        }
    }

    /// Searches `Header` in static and dynamic tables.
    pub(crate) fn search_header(&self, index: usize) -> Option<(Header, String)> {
        if index <= 61 {
            StaticTable::header(index)
        } else {
            self.dynamic.header(index - 62)
        }
    }

    /// Searches index in static and dynamic tables.
    pub(crate) fn index(&self, header: &Header, value: &str) -> Option<TableIndex> {
        match (
            StaticTable::index(header, value),
            self.dynamic.index(header, value),
        ) {
            (x @ Some(TableIndex::Header(_)), _) => x,
            (_, Some(TableIndex::Header(i))) => Some(TableIndex::Header(i + 62)),
            (x @ Some(TableIndex::HeaderName(_)), _) => x,
            (_, Some(TableIndex::HeaderName(i))) => Some(TableIndex::Header(i + 62)),
            _ => None,
        }
    }
}

pub(crate) enum TableIndex {
    Header(usize),
    HeaderName(usize),
}

/// The [`Dynamic Table`][dynamic_table] implementation of [HPACK].
///
/// [dynamic_table]: https://httpwg.org/specs/rfc7541.html#dynamic.table
/// [HPACK]: https://httpwg.org/specs/rfc7541.html
///
/// # Introduction
/// The dynamic table consists of a list of header fields maintained in
/// first-in, first-out order. The first and newest entry in a dynamic table is
/// at the lowest index, and the oldest entry of a dynamic table is at the
/// highest index.
///
/// The dynamic table is initially empty. Entries are added as each header block
/// is decompressed.
///
/// The dynamic table can contain duplicate entries (i.e., entries with the same
/// name and same value). Therefore, duplicate entries MUST NOT be treated as an
/// error by a decoder.
///
/// The encoder decides how to update the dynamic table and as such can control
/// how much memory is used by the dynamic table. To limit the memory
/// requirements of the decoder, the dynamic table size is strictly bounded.
///
/// The decoder updates the dynamic table during the processing of a list of
/// header field representations.
pub(crate) struct DynamicTable {
    queue: VecDeque<(Header, String)>,
    curr_size: usize,
    max_size: usize,
}

impl DynamicTable {
    /// Creates a `Dynamic Table` based on the size limit.
    pub(crate) fn with_max_size(max_size: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            curr_size: 0,
            max_size,
        }
    }

    pub(crate) fn curr_size(&self) -> usize {
        self.curr_size
    }

    pub(crate) fn max_size(&self) -> usize {
        self.max_size
    }

    /// Gets a `Header` by the given index.
    pub(crate) fn header_name(&self, index: usize) -> Option<Header> {
        self.queue.get(index).map(|(h, _)| h.clone())
    }

    /// Gets a `Header` and a value by the given index.
    pub(crate) fn header(&self, index: usize) -> Option<(Header, String)> {
        self.queue.get(index).cloned()
    }

    /// Updates `DynamicTable` by a given `Header` and value pair.
    pub(crate) fn update(&mut self, header: Header, value: String) {
        // RFC7541-4.1: The additional 32 octets account for an estimated
        // overhead associated with an entry. For example, an entry
        // structure using two 64-bit pointers to reference the name and the
        // value of the entry and two 64-bit integers for counting the
        // number of references to the name and value would have 32 octets
        // of overhead.
        self.curr_size += header.len() + value.len() + 32;
        self.queue.push_front((header, value));
        self.fit_size();
    }

    /// Updates `DynamicTable`'s size.
    pub(crate) fn update_size(&mut self, max_size: usize) {
        self.max_size = max_size;
        self.fit_size();
    }

    /// Adjusts dynamic table content to fit its size.
    fn fit_size(&mut self) {
        while self.curr_size > self.max_size && !self.queue.is_empty() {
            let (key, string) = self.queue.pop_back().unwrap();
            self.curr_size -= key.len() + string.len() + 32;
        }
    }

    /// Tries get the index of a `Header`.
    fn index(&self, header: &Header, value: &str) -> Option<TableIndex> {
        let mut index = None;
        for (n, (h, v)) in self.queue.iter().enumerate() {
            match (header == h, value == v, &index) {
                (true, true, _) => return Some(TableIndex::Header(n)),
                (true, false, None) => index = Some(TableIndex::HeaderName(n)),
                _ => {}
            }
        }
        index
    }
}

/// The [`Static Table`][static_table] implementation of [HPACK].
///
/// [static_table]: https://httpwg.org/specs/rfc7541.html#static.table
/// [HPACK]: https://httpwg.org/specs/rfc7541.html
///
/// # Introduction
/// The static table consists of a predefined static list of header fields.
///
/// # List
/// | Index | Header Name                   | Header Value  |
/// | :---: | :---:                         | :---:         |
/// | 1     | :authority                    |               |
/// | 2     | :method                       | GET           |
/// | 3     | :method                       | POST          |
/// | 4     | :path                         | /             |
/// | 5     | :path                         | /index.html   |
/// | 6     | :scheme                       | http          |
/// | 7     | :scheme                       | https         |
/// | 8     | :status                       | 200           |
/// | 9     | :status                       | 204           |
/// | 10    | :status                       | 206           |
/// | 11    | :status                       | 304           |
/// | 12    | :status                       | 400           |
/// | 13    | :status                       | 404           |
/// | 14    | :status                       | 500           |
/// | 15    | accept-charset                |               |
/// | 16    | accept-encoding               | gzip, deflate |
/// | 17    | accept-language               |               |
/// | 18    | accept-ranges                 |               |
/// | 19    | accept                        |               |
/// | 20    | access-control-allow-origin   |               |
/// | 21    | age                           |               |
/// | 22    | allow                         |               |
/// | 23    | authorization                 |               |
/// | 24    | cache-control                 |               |
/// | 25    | content-disposition           |               |
/// | 26    | content-encoding              |               |
/// | 27    | content-language              |               |
/// | 28    | content-length                |               |
/// | 29    | content-location              |               |
/// | 30    | content-range                 |               |
/// | 31    | content-type                  |               |
/// | 32    | cookie                        |               |
/// | 33    | date                          |               |
/// | 34    | etag                          |               |
/// | 35    | expect                        |               |
/// | 36    | expires                       |               |
/// | 37    | from                          |               |
/// | 38    | host                          |               |
/// | 39    | if-match                      |               |
/// | 40    | if-modified-since             |               |
/// | 41    | if-none-match                 |               |
/// | 42    | if-range                      |               |
/// | 43    | if-unmodified-since           |               |
/// | 44    | last-modified                 |               |
/// | 45    | link                          |               |
/// | 46    | location                      |               |
/// | 47    | max-forwards                  |               |
/// | 48    | proxy-authenticate            |               |
/// | 49    | proxy-authorization           |               |
/// | 50    | range                         |               |
/// | 51    | referer                       |               |
/// | 52    | refresh                       |               |
/// | 53    | retry-after                   |               |
/// | 54    | server                        |               |
/// | 55    | set-cookie                    |               |
/// | 56    | strict-transport-security     |               |
/// | 57    | transfer-encoding             |               |
/// | 58    | user-agent                    |               |
/// | 59    | vary                          |               |
/// | 60    | via                           |               |
/// | 61    | www-authenticate              |               |
struct StaticTable;

impl StaticTable {
    /// Gets a `Header` by the given index.
    fn header_name(index: usize) -> Option<Header> {
        match index {
            1 => Some(Header::Authority),
            2..=3 => Some(Header::Method),
            4..=5 => Some(Header::Path),
            6..=7 => Some(Header::Scheme),
            8..=14 => Some(Header::Status),
            15 => Some(Header::Other(String::from("accept-charset"))),
            16 => Some(Header::Other(String::from("accept-encoding"))),
            17 => Some(Header::Other(String::from("accept-language"))),
            18 => Some(Header::Other(String::from("accept-ranges"))),
            19 => Some(Header::Other(String::from("accept"))),
            20 => Some(Header::Other(String::from("access-control-allow-origin"))),
            21 => Some(Header::Other(String::from("age"))),
            22 => Some(Header::Other(String::from("allow"))),
            23 => Some(Header::Other(String::from("authorization"))),
            24 => Some(Header::Other(String::from("cache-control"))),
            25 => Some(Header::Other(String::from("content-disposition"))),
            26 => Some(Header::Other(String::from("content-encoding"))),
            27 => Some(Header::Other(String::from("content-language"))),
            28 => Some(Header::Other(String::from("content-length"))),
            29 => Some(Header::Other(String::from("content-location"))),
            30 => Some(Header::Other(String::from("content-range"))),
            31 => Some(Header::Other(String::from("content-type"))),
            32 => Some(Header::Other(String::from("cookie"))),
            33 => Some(Header::Other(String::from("date"))),
            34 => Some(Header::Other(String::from("etag"))),
            35 => Some(Header::Other(String::from("expect"))),
            36 => Some(Header::Other(String::from("expires"))),
            37 => Some(Header::Other(String::from("from"))),
            38 => Some(Header::Other(String::from("host"))),
            39 => Some(Header::Other(String::from("if-match"))),
            40 => Some(Header::Other(String::from("if-modified-since"))),
            41 => Some(Header::Other(String::from("if-none-match"))),
            42 => Some(Header::Other(String::from("if-range"))),
            43 => Some(Header::Other(String::from("if-unmodified-since"))),
            44 => Some(Header::Other(String::from("last-modified"))),
            45 => Some(Header::Other(String::from("link"))),
            46 => Some(Header::Other(String::from("location"))),
            47 => Some(Header::Other(String::from("max-forwards"))),
            48 => Some(Header::Other(String::from("proxy-authenticate"))),
            49 => Some(Header::Other(String::from("proxy-authorization"))),
            50 => Some(Header::Other(String::from("range"))),
            51 => Some(Header::Other(String::from("referer"))),
            52 => Some(Header::Other(String::from("refresh"))),
            53 => Some(Header::Other(String::from("retry-after"))),
            54 => Some(Header::Other(String::from("server"))),
            55 => Some(Header::Other(String::from("set-cookie"))),
            56 => Some(Header::Other(String::from("strict-transport-security"))),
            57 => Some(Header::Other(String::from("transfer-encoding"))),
            58 => Some(Header::Other(String::from("user-agent"))),
            59 => Some(Header::Other(String::from("vary"))),
            60 => Some(Header::Other(String::from("via"))),
            61 => Some(Header::Other(String::from("www-authenticate"))),
            _ => None,
        }
    }

    /// Tries to get a `Header` and a value by the given index.
    fn header(index: usize) -> Option<(Header, String)> {
        match index {
            2 => Some((Header::Method, String::from("GET"))),
            3 => Some((Header::Method, String::from("POST"))),
            4 => Some((Header::Path, String::from("/"))),
            5 => Some((Header::Path, String::from("/index.html"))),
            6 => Some((Header::Scheme, String::from("http"))),
            7 => Some((Header::Scheme, String::from("https"))),
            8 => Some((Header::Status, String::from("200"))),
            9 => Some((Header::Status, String::from("204"))),
            10 => Some((Header::Status, String::from("206"))),
            11 => Some((Header::Status, String::from("304"))),
            12 => Some((Header::Status, String::from("400"))),
            13 => Some((Header::Status, String::from("404"))),
            14 => Some((Header::Status, String::from("500"))),
            16 => Some((
                Header::Other(String::from("accept-encoding")),
                String::from("gzip, deflate"),
            )),
            _ => None,
        }
    }

    /// Tries to get a `Index` by the given header and value.
    fn index(header: &Header, value: &str) -> Option<TableIndex> {
        // TODO: 优化此处的比较逻辑，考虑使用单例哈希表。
        match (header, value) {
            (Header::Authority, _) => Some(TableIndex::HeaderName(1)),
            (Header::Method, "GET") => Some(TableIndex::Header(2)),
            (Header::Method, "POST") => Some(TableIndex::Header(3)),
            (Header::Method, _) => Some(TableIndex::HeaderName(2)),
            (Header::Path, "/") => Some(TableIndex::Header(4)),
            (Header::Path, "/index.html") => Some(TableIndex::Header(5)),
            (Header::Path, _) => Some(TableIndex::HeaderName(4)),
            (Header::Scheme, "http") => Some(TableIndex::Header(6)),
            (Header::Scheme, "https") => Some(TableIndex::Header(7)),
            (Header::Scheme, _) => Some(TableIndex::HeaderName(6)),
            (Header::Status, "200") => Some(TableIndex::Header(8)),
            (Header::Status, "204") => Some(TableIndex::Header(9)),
            (Header::Status, "206") => Some(TableIndex::Header(10)),
            (Header::Status, "304") => Some(TableIndex::Header(11)),
            (Header::Status, "400") => Some(TableIndex::Header(12)),
            (Header::Status, "404") => Some(TableIndex::Header(13)),
            (Header::Status, "500") => Some(TableIndex::Header(14)),
            (Header::Status, _) => Some(TableIndex::HeaderName(8)),
            (Header::Other(s), v) => match (s.as_str(), v) {
                ("accept-charset", _) => Some(TableIndex::HeaderName(15)),
                ("accept-encoding", "gzip, deflate") => Some(TableIndex::Header(16)),
                ("accept-encoding", _) => Some(TableIndex::HeaderName(16)),
                ("accept-language", _) => Some(TableIndex::HeaderName(17)),
                ("accept-ranges", _) => Some(TableIndex::HeaderName(18)),
                ("accept", _) => Some(TableIndex::HeaderName(19)),
                ("access-control-allow-origin", _) => Some(TableIndex::HeaderName(20)),
                ("age", _) => Some(TableIndex::HeaderName(21)),
                ("allow", _) => Some(TableIndex::HeaderName(22)),
                ("authorization", _) => Some(TableIndex::HeaderName(23)),
                ("cache-control", _) => Some(TableIndex::HeaderName(24)),
                ("content-disposition", _) => Some(TableIndex::HeaderName(25)),
                ("content-encoding", _) => Some(TableIndex::HeaderName(26)),
                ("content-language", _) => Some(TableIndex::HeaderName(27)),
                ("content-length", _) => Some(TableIndex::HeaderName(28)),
                ("content-location", _) => Some(TableIndex::HeaderName(29)),
                ("content-range", _) => Some(TableIndex::HeaderName(30)),
                ("content-type", _) => Some(TableIndex::HeaderName(31)),
                ("cookie", _) => Some(TableIndex::HeaderName(32)),
                ("date", _) => Some(TableIndex::HeaderName(33)),
                ("etag", _) => Some(TableIndex::HeaderName(34)),
                ("expect", _) => Some(TableIndex::HeaderName(35)),
                ("expires", _) => Some(TableIndex::HeaderName(36)),
                ("from", _) => Some(TableIndex::HeaderName(37)),
                ("host", _) => Some(TableIndex::HeaderName(38)),
                ("if-match", _) => Some(TableIndex::HeaderName(39)),
                ("if-modified-since", _) => Some(TableIndex::HeaderName(40)),
                ("if-none-match", _) => Some(TableIndex::HeaderName(41)),
                ("if-range", _) => Some(TableIndex::HeaderName(42)),
                ("if-unmodified-since", _) => Some(TableIndex::HeaderName(43)),
                ("last-modified", _) => Some(TableIndex::HeaderName(44)),
                ("link", _) => Some(TableIndex::HeaderName(45)),
                ("location", _) => Some(TableIndex::HeaderName(46)),
                ("max-forwards", _) => Some(TableIndex::HeaderName(47)),
                ("proxy-authenticate", _) => Some(TableIndex::HeaderName(48)),
                ("proxy-authorization", _) => Some(TableIndex::HeaderName(49)),
                ("range", _) => Some(TableIndex::HeaderName(50)),
                ("referer", _) => Some(TableIndex::HeaderName(51)),
                ("refresh", _) => Some(TableIndex::HeaderName(52)),
                ("retry-after", _) => Some(TableIndex::HeaderName(53)),
                ("server", _) => Some(TableIndex::HeaderName(54)),
                ("set-cookie", _) => Some(TableIndex::HeaderName(55)),
                ("strict-transport-security", _) => Some(TableIndex::HeaderName(56)),
                ("transfer-encoding", _) => Some(TableIndex::HeaderName(57)),
                ("user-agent", _) => Some(TableIndex::HeaderName(58)),
                ("vary", _) => Some(TableIndex::HeaderName(59)),
                ("via", _) => Some(TableIndex::HeaderName(60)),
                ("www-authenticate", _) => Some(TableIndex::HeaderName(61)),
                _ => None,
            },
        }
    }
}

/// Possible header types in `Dynamic Table` and `Static Table`.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Header {
    Authority,
    Method,
    Path,
    Scheme,
    Status,
    Other(String),
}

impl Header {
    pub(crate) fn len(&self) -> usize {
        match self {
            // 10 is the length of ":authority".
            Header::Authority => 10,
            // 7 is the length of ":method".
            Header::Method => 7,
            // 5 is the length of ":path".
            Header::Path => 5,
            // 7 is the length of "scheme".
            Header::Scheme => 7,
            // 7 is the length of "status".
            Header::Status => 7,
            Header::Other(s) => s.len(),
        }
    }

    pub(crate) fn into_string(self) -> String {
        match self {
            Header::Authority => String::from(":authority"),
            Header::Method => String::from(":method"),
            Header::Path => String::from(":path"),
            Header::Scheme => String::from(":scheme"),
            Header::Status => String::from(":status"),
            Header::Other(s) => s,
        }
    }
}

#[cfg(test)]
mod ut_dynamic_table {
    use crate::h2::hpack::table::{DynamicTable, Header, StaticTable};

    /// UT test cases for `DynamicTable::with_max_size`.
    ///
    /// # Brief
    /// 1. Calls `DynamicTable::with_max_size` to create a `DynamicTable`.
    /// 2. Checks the results.
    #[test]
    fn ut_dynamic_table_with_max_size() {
        let table = DynamicTable::with_max_size(4096);
        assert_eq!(table.queue.len(), 0);
        assert_eq!(table.curr_size, 0);
        assert_eq!(table.max_size, 4096);
    }

    /// UT test cases for `DynamicTable::header_name`.
    ///
    /// # Brief
    /// 1. Creates a `DynamicTable`.
    /// 2. Calls `DynamicTable::header_name` to get a header name.
    /// 3. Checks the results.
    #[test]
    fn ut_dynamic_table_header_name() {
        let mut table = DynamicTable::with_max_size(52);
        assert!(table.header_name(0).is_none());

        assert!(table.header_name(0).is_none());
        table.update(Header::Authority, String::from("Authority"));
        match table.header_name(0) {
            Some(Header::Authority) => {}
            _ => panic!("DynamicTable::header_name() failed!"),
        }
    }

    /// UT test cases for `DynamicTable::header`.
    ///
    /// # Brief
    /// 1. Creates a `DynamicTable`.
    /// 2. Calls `DynamicTable::header` to get a header and a value.
    /// 3. Checks the results.
    #[test]
    fn ut_dynamic_table_header() {
        let mut table = DynamicTable::with_max_size(52);
        assert!(table.header(0).is_none());

        assert!(table.header(0).is_none());
        table.update(Header::Authority, String::from("Authority"));
        match table.header(0) {
            Some((Header::Authority, x)) if x == *"Authority" => {}
            _ => panic!("DynamicTable::header() failed!"),
        }
    }

    /// UT test cases for `DynamicTable::update`.
    ///
    /// # Brief
    /// 1. Creates a `DynamicTable`.
    /// 2. Calls `DynamicTable::update` to insert a header and a value.
    /// 3. Checks the results.
    #[test]
    fn ut_dynamic_table_update() {
        let mut table = DynamicTable::with_max_size(52);
        table.update(Header::Authority, String::from("Authority"));
        assert_eq!(table.queue.len(), 1);
        match table.header(0) {
            Some((Header::Authority, x)) if x == *"Authority" => {}
            _ => panic!("DynamicTable::header() failed!"),
        }

        table.update(Header::Method, String::from("Method"));
        assert_eq!(table.queue.len(), 1);
        match table.header(0) {
            Some((Header::Method, x)) if x == *"Method" => {}
            _ => panic!("DynamicTable::header() failed!"),
        }
    }

    /// UT test cases for `DynamicTable::update_size`.
    ///
    /// # Brief
    /// 1. Creates a `DynamicTable`.
    /// 2. Calls `DynamicTable::update_size` to update its max size.
    /// 3. Checks the results.
    #[test]
    fn ut_dynamic_table_update_size() {
        let mut table = DynamicTable::with_max_size(52);
        table.update(Header::Authority, String::from("Authority"));
        assert_eq!(table.queue.len(), 1);
        match table.header(0) {
            Some((Header::Authority, x)) if x == *"Authority" => {}
            _ => panic!("DynamicTable::header() failed!"),
        }

        table.update_size(0);
        assert_eq!(table.queue.len(), 0);
        assert!(table.header(0).is_none());
    }

    /// UT test cases for `StaticTable::header_name` and `StaticTable::header`.
    ///
    /// # Brief
    /// 1. Iterates over a range of indices, testing both
    ///    `StaticTable::header_name` and `StaticTable::header`.
    /// 2. Verifies the presence or absence of header names and headers based on
    ///    the given index.
    #[test]
    fn ut_static_table() {
        // Checking header names for indices 1 to 64
        for index in 1..65 {
            if index < 62 {
                assert!(StaticTable::header_name(index).is_some())
            } else {
                assert!(StaticTable::header_name(index).is_none())
            }
        }

        // Checking headers for indices 2 to 19
        for index in 2..20 {
            if index < 17 && index != 15 {
                assert!(StaticTable::header(index).is_some())
            } else {
                assert!(StaticTable::header(index).is_none())
            }
        }
    }
}
