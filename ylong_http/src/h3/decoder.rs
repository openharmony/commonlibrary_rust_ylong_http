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

use crate::h3::frame::Headers;
use crate::h3::parts::Parts;
use crate::h3::qpack::error::H3Error_QPACK;
use crate::h3::qpack::table::DynamicTable;
use crate::h3::qpack::{FiledLines, QpackDecoder};

pub struct FrameDecoder<'a> {
    qpack_decoder: QpackDecoder<'a>,
    headers: Parts,
    qpack_encoder_buffer: Vec<u8>,
    remaining_qpack_payload: usize,
    stream_id: usize,
}

impl<'a> FrameDecoder<'a> {
    pub(crate) fn new(
        field_list_size: usize,
        table: &'a mut DynamicTable,
        stream_id: usize,
    ) -> Self {
        let frame_decoder = Self {
            qpack_decoder: QpackDecoder::new(field_list_size, table),
            headers: Parts::new(),
            qpack_encoder_buffer: vec![0; 16383],
            remaining_qpack_payload: 0,
            stream_id: stream_id,
        };
        frame_decoder
    }

    /// User call `decode_header` to decode headers.
    pub(crate) fn decode_header(&mut self, headers_payload: &[u8]) -> Result<(), H3Error_QPACK> {
        self.qpack_decoder.decode_repr(&headers_payload)
    }

    /// User call `finish_decode_header` to finish decode a stream.
    pub(crate) fn finish_decode_header(&mut self) {
        let results = self.qpack_decoder.finish(
            self.stream_id,
            &mut self.qpack_encoder_buffer[self.remaining_qpack_payload..],
        );
        if let Ok((header, cur_)) = results {
            self.headers = header;
            if let Some(cur) = cur_ {
                self.remaining_qpack_payload += cur;
            }
        }
    }

    /// User call `decode_qpack_ins` to decode peer's qpack_encoder_stream.
    pub(crate) fn decode_qpack_ins(&mut self, qpack_ins: &[u8]) -> Result<(), H3Error_QPACK> {
        self.qpack_decoder.decode_ins(&qpack_ins)
    }
}
#[cfg(test)]
mod ut_headers_decode {
    use crate::h3::decoder::FrameDecoder;
    use crate::h3::qpack::table::DynamicTable;
    use crate::h3::qpack::QpackDecoder;
    use crate::test_util::decode;
    #[test]
    fn literal_field_line_with_name_reference() {
        println!("run literal_field_line_with_name_reference");
        let mut table = DynamicTable::with_empty();
        table.update_size(1024);
        let mut f_decoder = FrameDecoder::new(16383, &mut table, 0);
        f_decoder.decode_header(&decode("0000510b2f696e6465782e68746d6c").unwrap());
        f_decoder.finish_decode_header();
        let (pseudo, map) = f_decoder.headers.into_parts();
        assert_eq!(pseudo.path, Some(String::from("/index.html")));
        println!("passed");
    }
}
