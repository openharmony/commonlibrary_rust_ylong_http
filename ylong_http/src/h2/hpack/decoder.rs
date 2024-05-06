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

use core::mem::take;

use crate::h2::error::ErrorCode;
use crate::h2::hpack::representation::{
    Name, ReprDecStateHolder, ReprDecodeState, ReprDecoder, Representation,
};
use crate::h2::hpack::table::{DynamicTable, Header, TableSearcher};
use crate::h2::{H2Error, Parts};

// A structure used to store header lines and octets lengths of header lines.
struct HeaderLines {
    parts: Parts,
    header_size: usize,
}

/// Decoder implementation of [`HPACK`].
///
/// [`HPACK`]: https://httpwg.org/specs/rfc7541.html
pub(crate) struct HpackDecoder {
    header_list_size: usize,
    table: DynamicTable,
    lines: HeaderLines,
    holder: ReprDecStateHolder,
}

impl HpackDecoder {
    /// Creates a `HpackDecoder` with the given max size.
    pub(crate) fn with_max_size(header_table_size: usize, max_header_list_size: usize) -> Self {
        Self {
            header_list_size: max_header_list_size,
            table: DynamicTable::with_max_size(header_table_size),
            lines: HeaderLines {
                parts: Parts::new(),
                header_size: 0,
            },
            holder: ReprDecStateHolder::new(),
        }
    }

    /// Users can call `decode` multiple times to decode the byte stream in
    /// segments.
    pub(crate) fn decode(&mut self, buf: &[u8]) -> Result<(), H2Error> {
        // Initialize ReprDecoder.
        let mut decoder = ReprDecoder::new(buf);
        decoder.load(&mut self.holder);

        let mut updater = Updater::new(&mut self.table, &mut self.lines, self.header_list_size);
        loop {
            match decoder.decode()? {
                // If a `Repr` is decoded, the `Updater` updates it immediately.
                Some(repr) => updater.update(repr)?,
                // If no `Repr` is decoded at this time, the intermediate result
                // needs to be saved.
                None => {
                    decoder.save(&mut self.holder);
                    return Ok(());
                }
            }
        }
    }

    /// Users call `finish` to stop decoding and get the result.
    pub(crate) fn finish(&mut self) -> Result<Parts, H2Error> {
        if !self.holder.is_empty() {
            return Err(H2Error::ConnectionError(ErrorCode::CompressionError));
        }
        self.lines.header_size = 0;
        Ok(take(&mut self.lines.parts))
    }

    /// Update the SETTING_HEADER_LIST_SIZE
    pub(crate) fn update_header_list_size(&mut self, size: usize) {
        self.header_list_size = size
    }
}

/// `Updater` is used to update `DynamicTable` `PseudoHeaders` and
/// `HttpHeaderMap`.
struct Updater<'a> {
    header_list_size: usize,
    table: &'a mut DynamicTable,
    lines: &'a mut HeaderLines,
}

impl<'a> Updater<'a> {
    /// Creates a new `Updater`.
    fn new(
        table: &'a mut DynamicTable,
        lines: &'a mut HeaderLines,
        header_list_size: usize,
    ) -> Self {
        Self {
            table,
            lines,
            header_list_size,
        }
    }

    /// Updates the `DynamicTable` and `Parts`.
    fn update(&mut self, repr: Representation) -> Result<(), H2Error> {
        match repr {
            Representation::Indexed { index } => self.update_indexed(index),
            Representation::LiteralWithIndexing { name: n, value: v } => {
                self.update_literal_with_indexing(n, v)
            }
            Representation::LiteralWithoutIndexing { name: n, value: v } => {
                self.update_literal_without_indexing(n, v)
            }
            Representation::LiteralNeverIndexed { name: n, value: v } => {
                self.update_literal_never_indexing(n, v)
            }
            Representation::SizeUpdate { max_size } => {
                self.table.update_size(max_size);
                Ok(())
            }
        }
    }

    fn update_indexed(&mut self, index: usize) -> Result<(), H2Error> {
        let searcher = TableSearcher::new(self.table);
        let (h, v) = searcher
            .search_header(index)
            .ok_or(H2Error::ConnectionError(ErrorCode::CompressionError))?;
        self.check_header_list_size(&h, &v)?;
        self.lines.parts.update(h, v);
        Ok(())
    }

    fn update_literal_with_indexing(&mut self, name: Name, value: Vec<u8>) -> Result<(), H2Error> {
        let (h, v) = self.get_header_by_name_and_value(name, value)?;
        self.check_header_list_size(&h, &v)?;
        self.table.update(h.clone(), v.clone());
        self.lines.parts.update(h, v);
        Ok(())
    }

    fn update_literal_without_indexing(
        &mut self,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(), H2Error> {
        let (h, v) = self.get_header_by_name_and_value(name, value)?;
        self.check_header_list_size(&h, &v)?;
        self.lines.parts.update(h, v);
        Ok(())
    }

    // TODO: 支持 `LiteralNeverIndexed`.
    fn update_literal_never_indexing(&mut self, name: Name, value: Vec<u8>) -> Result<(), H2Error> {
        self.update_literal_without_indexing(name, value)
    }

    fn check_header_list_size(&mut self, key: &Header, value: &str) -> Result<(), H2Error> {
        let line_size = header_line_length(key.len(), value.len());
        self.update_size(line_size);
        if self.lines.header_size > self.header_list_size {
            Err(H2Error::ConnectionError(ErrorCode::ConnectError))
        } else {
            Ok(())
        }
    }

    pub(crate) fn update_size(&mut self, addition: usize) {
        self.lines.header_size += addition;
    }

    fn get_header_by_name_and_value(
        &self,
        name: Name,
        value: Vec<u8>,
    ) -> Result<(Header, String), H2Error> {
        let h = match name {
            Name::Index(index) => {
                let searcher = TableSearcher::new(self.table);
                searcher
                    .search_header_name(index)
                    .ok_or(H2Error::ConnectionError(ErrorCode::CompressionError))?
            }
            Name::Literal(octets) => Header::Other(
                String::from_utf8(octets)
                    .map_err(|_| H2Error::ConnectionError(ErrorCode::CompressionError))?,
            ),
        };
        let v = String::from_utf8(value)
            .map_err(|_| H2Error::ConnectionError(ErrorCode::CompressionError))?;
        Ok((h, v))
    }
}

fn header_line_length(key_size: usize, value_size: usize) -> usize {
    key_size + value_size + 32
}

#[cfg(test)]
mod ut_hpack_decoder {
    use crate::h2::hpack::table::Header;
    use crate::h2::hpack::HpackDecoder;
    use crate::util::test_util::decode;

    const MAX_HEADER_LIST_SIZE: usize = 16 << 20;

    /// UT test cases for `HpackDecoder`.
    ///
    /// # Brief
    /// 1. Creates a `HpackDecoder`.
    /// 2. Calls `HpackDecoder::decode()` function, passing in the specified
    /// parameters.
    /// 3. Checks if the test results are correct.
    #[test]
    fn ut_hpack_decoder() {
        rfc7541_test_cases();
        slices_test_cases();

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

        macro_rules! check_map {
            ($map: expr, { $($(,)? $k: literal => $v: literal)* } $(,)?) => {
                $(
                    assert_eq!($map.get($k).unwrap().to_string().unwrap(), $v);
                )*
            }
        }

        macro_rules! check_table {
            (
                $hpack: expr, $size: expr,
                { $($(,)? $($k: literal)? $($k2: ident)? => $v: literal)* } $(,)?
            ) => {
                assert_eq!($hpack.table.curr_size(), $size);
                let mut _cnt = 0;
                $(

                    $(
                        match $hpack.table.header(_cnt) {
                            Some((Header::Other(k), v)) if k == $k && v == $v => {},
                            _ => panic!("DynamicTable::header() failed! (branch 1)"),
                        }
                    )?
                    $(
                        match $hpack.table.header(_cnt) {
                            Some((Header::$k2, v)) if v == $v => {},
                            _ => panic!("DynamicTable::header() failed! (branch 2)"),
                        }
                    )?
                    _cnt += 1;
                )*
            }
        }

        macro_rules! get_parts {
            ($hpack: expr $(, $input: literal)*) => {{
                $(
                    let text = decode($input).unwrap();
                    assert!($hpack.decode(text.as_slice()).is_ok());
                )*
                match $hpack.finish() {
                    Ok(parts) => parts,
                    Err(_) => panic!("HpackDecoder::finish() failed!"),
                }
            }};
        }

        macro_rules! hpack_test_case {
            (
                $hpack: expr $(, $input: literal)*,
                { $a: expr, $m: expr, $p: expr, $sc: expr, $st: expr },
                { $size: expr $(, $($k2: literal)? $($k3: ident)? => $v2: literal)* } $(,)?
            ) => {
                let mut _hpack = $hpack;
                let (pseudo, _) = get_parts!(_hpack $(, $input)*).into_parts();
                check_pseudo!(pseudo, { $a, $m, $p, $sc, $st });
                check_table!(_hpack, $size, { $($($k2)? $($k3)? => $v2)* });
            };

            (
                $hpack: expr $(, $input: literal)*,
                { $($(,)? $k1: literal => $v1: literal)* },
                { $size: expr $(, $($k2: literal)? $($k3: ident)? => $v2: literal)* } $(,)?
            ) => {
                let mut _hpack = $hpack;
                let (_, map) = get_parts!(_hpack $(, $input)*).into_parts();
                check_map!(map, { $($k1 => $v1)* });
                check_table!(_hpack, $size, { $($($k2)? $($k3)? => $v2)* });
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
                check_table!(_hpack, $size, { $($($k2)? $($k3)? => $v2)* });
            };
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.2.1. Literal Header Field with Indexing
            hpack_test_case!(
                HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE),
                "400a637573746f6d2d6b65790d637573746f6d2d686561646572",
                { "custom-key" => "custom-header" },
                { 55, "custom-key" => "custom-header" },
            );

            // C.2.2. Literal Header Field without Indexing
            hpack_test_case!(
                HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE),
                "040c2f73616d706c652f70617468",
                { None, None, Some("/sample/path"), None, None },
                { 0 }
            );

            // C.2.3. Literal Header Field Never Indexed
            hpack_test_case!(
                HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE),
                "100870617373776f726406736563726574",
                { "password" => "secret" },
                { 0 },
            );

            // C.2.4. Indexed Header Field
            hpack_test_case!(
                HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE),
                "82",
                { None, Some("GET"), None, None, None },
                { 0 }
            );

            // Request Examples without Huffman Coding.
            {
                let mut hpack_decoder = HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE);
                // C.3.1. First Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828684410f7777772e6578616d706c652e636f6d",
                    { Some("www.example.com"), Some("GET"), Some("/"), Some("http"), None },
                    { 57, Authority => "www.example.com" }
                );

                // C.3.2. Second Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828684be58086e6f2d6361636865",
                    { Some("www.example.com"), Some("GET"), Some("/"), Some("http"), None },
                    { "cache-control" => "no-cache" },
                    { 110, "cache-control" => "no-cache", Authority => "www.example.com" }
                );

                // C.3.3. Third Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
                    { Some("www.example.com"), Some("GET"), Some("/index.html"), Some("https"), None },
                    { "custom-key" => "custom-value" },
                    { 164, "custom-key" => "custom-value", "cache-control" => "no-cache", Authority => "www.example.com" }
                );
            }

            // C.4. Request Examples with Huffman Coding
            {
                let mut hpack_decoder = HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE);
                // C.4.1. First Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828684418cf1e3c2e5f23a6ba0ab90f4ff",
                    { Some("www.example.com"), Some("GET"), Some("/"), Some("http"), None },
                    { 57, Authority => "www.example.com" }
                );

                // C.4.2. Second Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828684be5886a8eb10649cbf",
                    { Some("www.example.com"), Some("GET"), Some("/"), Some("http"), None },
                    { "cache-control" => "no-cache" },
                    { 110, "cache-control" => "no-cache", Authority => "www.example.com" }
                );

                // C.4.3. Third Request
                hpack_test_case!(
                    &mut hpack_decoder,
                    "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
                    { Some("www.example.com"), Some("GET"), Some("/index.html"), Some("https"), None },
                    { "custom-key" => "custom-value" },
                    { 164, "custom-key" => "custom-value", "cache-control" => "no-cache", Authority => "www.example.com" }
                );
            }

            // C.5. Response Examples without Huffman Coding
            {
                let mut hpack_decoder = HpackDecoder::with_max_size(256, MAX_HEADER_LIST_SIZE);
                // C.5.1. First Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "4803333032580770726976617465611d\
                    4d6f6e2c203231204f63742032303133\
                    2032303a31333a323120474d546e1768\
                    747470733a2f2f7777772e6578616d70\
                    6c652e636f6d",
                    { None, None, None, None, Some("302") },
                    {
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private"
                    },
                    {
                        222,
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private",
                        Status => "302"
                    }
                );

                // C.5.2. Second Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "4803333037c1c0bf",
                    { None, None, None, None, Some("307") },
                    {
                        "cache-control" => "private",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "location" => "https://www.example.com"
                    },
                    {
                        222,
                        Status => "307",
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private"
                    }
                );

                // C.5.3. Third Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "88c1611d4d6f6e2c203231204f637420\
                    323031332032303a31333a323220474d\
                    54c05a04677a69707738666f6f3d4153\
                    444a4b48514b425a584f5157454f5049\
                    5541585157454f49553b206d61782d61\
                    67653d333630303b2076657273696f6e\
                    3d31",
                    { None, None, None, None, Some("200") },
                    {
                        "cache-control" => "private",
                        "date" => "Mon, 21 Oct 2013 20:13:22 GMT",
                        "location" => "https://www.example.com",
                        "content-encoding" => "gzip",
                        "set-cookie" => "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"
                    },
                    {
                        215,
                        "set-cookie" => "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1",
                        "content-encoding" => "gzip",
                        "date" => "Mon, 21 Oct 2013 20:13:22 GMT"
                    }
                );
            }

            // C.6. Response Examples with Huffman Coding
            {
                let mut hpack_decoder = HpackDecoder::with_max_size(256, MAX_HEADER_LIST_SIZE);
                // C.6.1. First Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "488264025885aec3771a4b6196d07abe\
                    941054d444a8200595040b8166e082a6\
                    2d1bff6e919d29ad171863c78f0b97c8\
                    e9ae82ae43d3",
                    { None, None, None, None, Some("302") },
                    {
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private"
                    },
                    {
                        222,
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private",
                        Status => "302"
                    }
                );

                // C.6.2. Second Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "4883640effc1c0bf",
                    { None, None, None, None, Some("307") },
                    {
                        "cache-control" => "private",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "location" => "https://www.example.com"
                    },
                    {
                        222,
                        Status => "307",
                        "location" => "https://www.example.com",
                        "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                        "cache-control" => "private"
                    }
                );

                // C.6.3. Third Response
                hpack_test_case!(
                    &mut hpack_decoder,
                    "88c16196d07abe941054d444a8200595\
                    040b8166e084a62d1bffc05a839bd9ab\
                    77ad94e7821dd7f2e6c7b335dfdfcd5b\
                    3960d5af27087f3672c1ab270fb5291f\
                    9587316065c003ed4ee5b1063d5007",
                    { None, None, None, None, Some("200") },
                    {
                        "cache-control" => "private",
                        "date" => "Mon, 21 Oct 2013 20:13:22 GMT",
                        "location" => "https://www.example.com",
                        "content-encoding" => "gzip",
                        "set-cookie" => "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"
                    },
                    {
                        215,
                        "set-cookie" => "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1",
                        "content-encoding" => "gzip",
                        "date" => "Mon, 21 Oct 2013 20:13:22 GMT"
                    }
                );
            }
        }

        fn slices_test_cases() {
            // C.2.1. Literal Header Field with Indexing
            hpack_test_case!(
                HpackDecoder::with_max_size(4096, MAX_HEADER_LIST_SIZE),
                "04", "0c", "2f", "73", "61", "6d", "70", "6c", "65", "2f", "70", "61", "74", "68",
                { None, None, Some("/sample/path"), None, None },
                { 0 }
            );

            // C.6.1. First Response
            hpack_test_case!(
                HpackDecoder::with_max_size(256, MAX_HEADER_LIST_SIZE),
                "488264025885aec3771a4b6196d07abe",
                "941054d444a8200595040b8166e082a6",
                "2d1bff6e919d29ad171863c78f0b97c8",
                "e9ae82ae43d3",
                { None, None, None, None, Some("302") },
                {
                    "location" => "https://www.example.com",
                    "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                    "cache-control" => "private"
                },
                {
                    222,
                    "location" => "https://www.example.com",
                    "date" => "Mon, 21 Oct 2013 20:13:21 GMT",
                    "cache-control" => "private",
                    Status => "302"
                }
            );
        }
    }
}
