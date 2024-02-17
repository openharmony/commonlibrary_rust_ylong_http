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

use crate::h2::hpack::representation::{ReprEncStateHolder, ReprEncodeState, ReprEncoder};
use crate::h2::hpack::table::{DynamicTable, Header};
use crate::h2::{Parts, PseudoHeaders};

/// Decoder implementation of [`HPACK`].
///
/// [`HPACK`]: https://httpwg.org/specs/rfc7541.html
// TODO: 增加 SizeUpdate 字段以支持用户动态变化 DynamicTable 大小。
pub(crate) struct HpackEncoder {
    table: DynamicTable,
    holder: ReprEncStateHolder,
}

impl HpackEncoder {
    /// Create a `HpackEncoder` with the given max size.
    pub(crate) fn with_max_size(max_size: usize) -> Self {
        Self {
            table: DynamicTable::with_max_size(max_size),
            holder: ReprEncStateHolder::new(),
        }
    }

    /// Set the `Parts` to be encoded.
    pub(crate) fn set_parts(&mut self, parts: Parts) {
        self.holder.set_parts(parts)
    }

    /// Users can call `encode` multiple times to encode the previously set
    /// `Parts` in segments.
    pub(crate) fn encode(&mut self, dst: &mut [u8]) -> usize {
        let mut encoder = ReprEncoder::new(&mut self.table);
        encoder.load(&mut self.holder);
        let size = encoder.encode(dst);
        if size == dst.len() {
            encoder.save(&mut self.holder);
        }
        size
    }

    /// Check the previously set `Parts` if encoding is complete.
    pub(crate) fn is_finished(&self) -> bool {
        self.holder.is_empty()
    }
}

#[cfg(test)]
mod ut_hpack_encoder {
    use crate::h2::hpack::table::Header;
    use crate::h2::hpack::HpackEncoder;
    use crate::h2::Parts;
    use crate::util::test_util::decode;

    #[test]
    fn ut_hpack_encoder() {
        rfc7541_test_cases();

        // In order to ensure that Header and Value are added in the order of
        // `RFC`, each time a Parts is generated separately and passed in
        macro_rules! hpack_test_cases {
            ($enc: expr, $len: expr, $res: literal, $size: expr , { $($h: expr, $v: expr $(,)?)*} $(,)?) => {
                let mut _encoder = $enc;
                let mut vec = [0u8; $len];
                let mut cur = 0;
                $(
                    let mut parts = Parts::new();
                    parts.update($h, $v);
                    _encoder.set_parts(parts);
                    cur += _encoder.encode(&mut vec[cur..]);
                )*
                assert_eq!(cur, $len);
                let result = decode($res).unwrap();
                assert_eq!(vec.as_slice(), result.as_slice());
                assert_eq!(_encoder.table.curr_size(), $size);
            }
        }

        /// The following test cases are from RFC7541.
        fn rfc7541_test_cases() {
            // C.2.1.  Literal Header Field with Indexing
            hpack_test_cases!(
                HpackEncoder::with_max_size(4096),
                26, "400a637573746f6d2d6b65790d637573746f6d2d686561646572", 55,
                {
                    Header::Other(String::from("custom-key")),
                    String::from("custom-header"),
                },
            );

            // TODO: C.2.2.  Literal Header Field without Indexing
            // TODO: C.2.3.  Literal Header Field Never Indexed

            // C.2.4.  Indexed Header Field
            hpack_test_cases!(
                HpackEncoder::with_max_size(4096),
                1, "82", 0,
                {
                    Header::Method,
                    String::from("GET"),
                },
            );

            // C.3.  Request Examples without Huffman Coding
            {
                let mut encoder = HpackEncoder::with_max_size(4096);
                // C.3.1.  First Request
                hpack_test_cases!(
                    &mut encoder,
                    20, "828684410f7777772e6578616d706c652e636f6d", 57,
                    {
                        Header::Method,
                        String::from("GET"),
                        Header::Scheme,
                        String::from("http"),
                        Header::Path,
                        String::from("/"),
                        Header::Authority,
                        String::from("www.example.com"),
                    },
                );

                // C.3.2.  Second Request
                hpack_test_cases!(
                    &mut encoder,
                    14, "828684be58086e6f2d6361636865", 110,
                    {
                        Header::Method,
                        String::from("GET"),
                        Header::Scheme,
                        String::from("http"),
                        Header::Path,
                        String::from("/"),
                        Header::Authority,
                        String::from("www.example.com"),
                        Header::Other(String::from("cache-control")),
                        String::from("no-cache"),
                    },
                );

                // C.3.3.  Third Request
                hpack_test_cases!(
                    &mut encoder,
                    29, "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565", 164,
                    {
                        Header::Method,
                        String::from("GET"),
                        Header::Scheme,
                        String::from("https"),
                        Header::Path,
                        String::from("/index.html"),
                        Header::Authority,
                        String::from("www.example.com"),
                        Header::Other(String::from("custom-key")),
                        String::from("custom-value"),
                    },
                );
            }

            // TODO: C.4.  Request Examples with Huffman Coding

            // C.5.  Response Examples without Huffman Coding
            {
                let mut encoder = HpackEncoder::with_max_size(256);
                // C.5.1.  First Response
                hpack_test_cases!(
                    &mut encoder,
                    70,
                    "4803333032580770726976617465611d\
                    4d6f6e2c203231204f63742032303133\
                    2032303a31333a323120474d546e1768\
                    747470733a2f2f7777772e6578616d70\
                    6c652e636f6d",
                    222,
                    {
                        Header::Status,
                        String::from("302"),
                        Header::Other(String::from("cache-control")),
                        String::from("private"),
                        Header::Other(String::from("date")),
                        String::from("Mon, 21 Oct 2013 20:13:21 GMT"),
                        Header::Other(String::from("location")),
                        String::from("https://www.example.com"),
                    },
                );

                // C.5.2.  Second Response
                hpack_test_cases!(
                    &mut encoder,
                    8, "4803333037c1c0bf", 222,
                    {
                        Header::Status,
                        String::from("307"),
                        Header::Other(String::from("cache-control")),
                        String::from("private"),
                        Header::Other(String::from("date")),
                        String::from("Mon, 21 Oct 2013 20:13:21 GMT"),
                        Header::Other(String::from("location")),
                        String::from("https://www.example.com"),
                    },
                );

                // C.5.3.  Third Response
                hpack_test_cases!(
                    &mut encoder,
                    98,
                    "88c1611d4d6f6e2c203231204f637420\
                    323031332032303a31333a323220474d\
                    54c05a04677a69707738666f6f3d4153\
                    444a4b48514b425a584f5157454f5049\
                    5541585157454f49553b206d61782d61\
                    67653d333630303b2076657273696f6e\
                    3d31",
                    215,
                    {
                        Header::Status,
                        String::from("200"),
                        Header::Other(String::from("cache-control")),
                        String::from("private"),
                        Header::Other(String::from("date")),
                        String::from("Mon, 21 Oct 2013 20:13:22 GMT"),
                        Header::Other(String::from("location")),
                        String::from("https://www.example.com"),
                        Header::Other(String::from("content-encoding")),
                        String::from("gzip"),
                        Header::Other(String::from("set-cookie")),
                        String::from("foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1"),
                    },
                );
            }

            // TODO: C.6.  Response Examples with Huffman Coding
        }
    }
}
