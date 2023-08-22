use std::collections::{HashMap, VecDeque};
use crate::h3::error::ErrorCode::QPACK_DECODER_STREAM_ERROR;
use crate::h3::error::H3Error;
use crate::h3::parts::Parts;
use crate::h3::qpack::table::DynamicTable;
use crate::h3::qpack::format::{ReprEncoder, ReprEncStateHolder};
use crate::h3::qpack::format::encoder::{DecInstDecoder, InstDecodeState, InstDecStateHolder};
use crate::h3::qpack::{DecoderInstruction, PrefixMask};
use crate::h3::qpack::integer::{Integer, IntegerEncoder};

pub(crate) struct QpackEncoder {
    table: DynamicTable,
    holder: ReprEncStateHolder,
    inst_holder: InstDecStateHolder,
    stream_reference: VecDeque<Option<usize>>,
    stream_id: usize,
    is_insert: bool,
    // if not insert to dynamic table, the required insert count will be 0.
    allow_post: bool,
    // allow reference to the inserting field default is false.
    draining_index: usize, // RFC9204-2.1.1.1. if index<draining_index, then execute Duplicate.
}

impl QpackEncoder {
    pub(crate) fn with_capacity(max_size: usize, encoder_buf: &mut [u8], stream_id: usize, allow_post: bool, draining_index: usize) -> (QpackEncoder, usize) {
        let mut s = Self {
            table: DynamicTable::with_capacity(max_size),
            holder: ReprEncStateHolder::new(),
            inst_holder: InstDecStateHolder::new(),
            stream_reference: VecDeque::new(),
            stream_id,
            is_insert: false,
            allow_post: allow_post,
            draining_index: draining_index,
        };
        let cur = EncoderInst::SetCap(SetCap::new(max_size)).encode(&mut encoder_buf[..]);
        (s, cur)
    }


    pub(crate) fn set_parts(&mut self, parts: Parts) {
        self.holder.set_parts(parts)
    }

    /// Users can call `decode_ins` multiple times to decode decoder instructions.
    pub(crate) fn decode_ins(&mut self, buf: &[u8]) -> Result<Option<DecoderInst>, H3Error> {
        let mut decoder = DecInstDecoder::new(buf);
        decoder.load(&mut self.inst_holder);
        loop {
            match decoder.decode()? {
                Some(inst) => {
                    match inst {
                        DecoderInstruction::Ack { stream_id } => {
                            println!("stream_id: {}", stream_id);
                            assert_eq!(stream_id, self.stream_id);
                            loop {// ack an field section's all index
                                let ack_index = self.stream_reference.pop_front();
                                if let Some(index) = ack_index {
                                    if index == None {
                                        break;// end of field section
                                    }
                                    if let Some(ind) = index {
                                        if let Some(count) = self.table.ref_count.get(&ind) {
                                            self.table.ref_count.insert(ind, count - 1);
                                        }
                                    }
                                } else {
                                    return Err(H3Error::ConnectionError(QPACK_DECODER_STREAM_ERROR));
                                }
                            }
                            self.table.known_received_count += 1;
                        }
                        DecoderInstruction::StreamCancel { stream_id } => {
                            println!("stream_id: {}", stream_id);
                            assert_eq!(stream_id, self.stream_id);
                            return Ok(Some(DecoderInst::StreamCancel));
                        }
                        DecoderInstruction::InsertCountIncrement { increment } => {
                            println!("increment: {}", increment);
                            self.table.known_received_count += increment;
                        }
                    }
                }
                None => return Ok(None),
            }
        }
    }

    /// Users can call `encode` multiple times to encode multiple complete field sections.
    pub(crate) fn encode(&mut self, encoder_buf: &mut [u8], stream_buf: &mut [u8]) -> (usize, usize, Option<([u8; 1024], usize)>) {
        let mut cur_prefix = 0;
        let mut cur_encoder = 0;
        let mut cur_stream = 0;
        if self.is_finished() {
            // denote an end of field section
            self.stream_reference.push_back(None);
            let mut wire_ric = 0;
            if self.is_insert {
                wire_ric = self.table.insert_count % (2 * self.table.max_entries()) + 1;
            }
            let mut prefix_buf = [0u8; 1024];
            cur_prefix += Integer::index(0x00, wire_ric, 0xff).encode(&mut prefix_buf[..]).unwrap_or(0);
            if self.table.known_received_count >= self.table.insert_count {
                cur_prefix += Integer::index(0x00, self.table.known_received_count - self.table.insert_count, 0x7f).encode(&mut prefix_buf[cur_prefix..]).unwrap_or(0);
            } else {
                cur_prefix += Integer::index(0x80, self.table.insert_count - self.table.known_received_count - 1, 0x7f).encode(&mut prefix_buf[cur_prefix..]).unwrap_or(0);
            }
            (cur_encoder, cur_stream, Some((prefix_buf, cur_prefix)))
        } else {
            let mut encoder = ReprEncoder::new(&mut self.table, self.draining_index);
            encoder.load(&mut self.holder);
            (cur_encoder, cur_stream) = encoder.encode(&mut encoder_buf[0..], &mut stream_buf[0..], &mut self.stream_reference, &mut self.is_insert, self.allow_post);
            (cur_encoder, cur_stream, None)
        }
    }

    /// Check the previously set `Parts` if encoding is complete.
    pub(crate) fn is_finished(&self) -> bool {
        self.holder.is_empty()
    }
}


pub(crate) enum DecoderInst {
    Ack,
    StreamCancel,
    InsertCountIncrement,
}

pub(crate) enum EncoderInst {
    SetCap(SetCap),
}

impl EncoderInst {
    pub(crate) fn encode(self, encoder_buf: &mut [u8]) -> usize {
        let resut = match self {
            Self::SetCap(s) => s.encode(encoder_buf),
            // _ => panic!("not support"),
        };
        match resut {
            Ok(size) => size,
            Err(e) => panic!("encode error"),
        }
    }
}


pub(crate) struct SetCap {
    capacity: Integer,
}

impl SetCap {
    fn from(capacity: Integer) -> Self {
        Self { capacity }
    }

    fn new(capacity: usize) -> Self {
        Self { capacity: Integer::index(0x20, capacity, PrefixMask::SETCAP.0) }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, EncoderInst> {
        self.capacity
            .encode(dst)
            .map_err(|e| EncoderInst::SetCap(SetCap::from(e)))
    }
}


#[cfg(test)]
mod ut_qpack_encoder {
    use crate::h3::parts::Parts;
    use crate::h3::qpack::encoder;
    use crate::h3::qpack::encoder::{QpackEncoder};
    use crate::h3::qpack::table::Field;
    use crate::test_util::decode;

    #[test]
    fn ut_qpack_encoder() {
        rfc9204_test_cases();
        macro_rules! qpack_test_cases {
            ($enc: expr,$encoder_buf:expr,$encoder_cur:expr, $len: expr, $res: literal,$encoder_res: literal, $size: expr, { $($h: expr, $v: expr $(,)?)*} $(,)?) => {
                let mut _encoder = $enc;
                let mut stream_buf = [0u8; $len];
                let mut stream_cur = 0;
                println!("size: {}", _encoder.table.size());
                $(
                    let mut parts = Parts::new();
                    parts.update($h, $v);
                    _encoder.set_parts(parts);
                    let (mut cur1,mut cur2,_) = _encoder.encode(&mut $encoder_buf[$encoder_cur..],&mut stream_buf[stream_cur..]);
                    $encoder_cur += cur1;
                    stream_cur += cur2;
                )*
                let (mut cur1,mut cur2,mut prefix) = _encoder.encode(&mut $encoder_buf[$encoder_cur..],&mut stream_buf[stream_cur..]);
                $encoder_cur += cur1;
                stream_cur += cur2;
                if let Some((prefix_buf,cur_prefix)) = prefix{
                    stream_buf.copy_within(0..stream_cur,cur_prefix);
                    stream_buf[..cur_prefix].copy_from_slice(&prefix_buf[..cur_prefix]);
                    stream_cur += cur_prefix;
                }
                let result = decode($res).unwrap();
                println!("stream_cur: {:#?}", stream_cur);
                println!("stream buf: {:#?}", stream_buf);
                println!("stream result: {:#?}", result);
                println!("encoder buf: {:#?}", $encoder_buf[..$encoder_cur].to_vec().as_slice());
                if let Some(res) = decode($encoder_res){
                    println!("encoder result: {:#?}", res);
                    assert_eq!($encoder_buf[..$encoder_cur].to_vec().as_slice(), res.as_slice());
                }
                assert_eq!(stream_cur, $len);
                assert_eq!(stream_buf.as_slice(), result.as_slice());
                assert_eq!(_encoder.table.size(), $size);
            }
        }

        /// The following test cases are from RFC9204.
        fn rfc9204_test_cases() {
            literal_field_line_with_name_reference();
            dynamic_table();
            speculative_insert();
            duplicate_instruction_stream_cancellation();
            dynamic_table_insert_eviction();

            /// The encoder sends an encoded field section containing a literal representation of a field with a static name reference.
            fn literal_field_line_with_name_reference() {
                let mut encoder_buf = [0u8; 1024];
                let (mut encoder, mut encoder_cur) = QpackEncoder::with_capacity(0, &mut encoder_buf[..], 0, false,0);
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

            ///The encoder sets the dynamic table capacity, inserts a header with a dynamic name
            /// reference, then sends a potentially blocking, encoded field section referencing
            /// this new entry. The decoder acknowledges processing the encoded field section,
            /// which implicitly acknowledges all dynamic table insertions up to the Required
            /// Insert Count.
            fn dynamic_table() {
                let mut encoder_buf = [0u8; 1024];
                let (mut encoder, mut encoder_cur) = QpackEncoder::with_capacity(220, &mut encoder_buf[..], 0, true,0);
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

            ///The encoder inserts a header into the dynamic table with a literal name.
            /// The decoder acknowledges receipt of the entry. The encoder does not send any
            /// encoded field sections.
            fn speculative_insert() {
                let mut encoder_buf = [0u8; 1024];
                let (mut encoder, mut encoder_cur) = QpackEncoder::with_capacity(220, &mut encoder_buf[..], 0, true,0);
                encoder_cur = 0;
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

            /// ## About the setting of Base in RFC 9204
            /// (1). From RFC: If the encoder inserted entries in the dynamic table while encoding the field
            /// section and is referencing them, Required Insert Count will be greater than the Base.
            /// (2). From RFC: An encoder that produces table updates before encoding a field section might
            /// set Base to the value of Required Insert Count. In such a case, both the Sign bit
            /// and the Delta Base will be set to zero.
            /// ## My implementation
            /// I utilize the above condition (1), and the base is set to the count of decoder known entries(known receive count).
            /// Just As the above test: `dynamic_table()` is same as RFC 9204.
            /// But, the following test utilized the (2), So, it is something different from RFC 9204. because the above reason.

            fn duplicate_instruction_stream_cancellation(){
                let mut encoder_buf = [0u8; 1024];
                let (mut encoder, mut encoder_cur) = QpackEncoder::with_capacity(4096, &mut encoder_buf[..], 0, true,1);
                encoder.table.update(Field::Authority,String::from("www.example.com"));
                encoder.table.update(Field::Path,String::from("/sample/path"));
                encoder.table.update(Field::Other(String::from("custom-key")),String::from("custom-value"));
                encoder.table.ref_count.insert(0, 0); //Acked
                encoder.table.ref_count.insert(1, 0); //Acked
                encoder.table.ref_count.insert(2, 0); //Acked
                encoder.table.known_received_count = 3;
                encoder_cur = 0;
                qpack_test_cases!(
                    encoder,
                    encoder_buf,
                    encoder_cur,
                    5, "058010c180",
                    "02",
                    217,
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

            fn dynamic_table_insert_eviction(){
                let mut encoder_buf = [0u8; 1024];
                let (mut encoder, mut encoder_cur) = QpackEncoder::with_capacity(4096, &mut encoder_buf[..], 0, true,1);
                encoder.table.update(Field::Authority,String::from("www.example.com"));
                encoder.table.update(Field::Path,String::from("/sample/path"));
                encoder.table.update(Field::Other(String::from("custom-key")),String::from("custom-value"));
                encoder.table.update(Field::Authority,String::from("www.example.com"));
                encoder.table.ref_count.insert(0, 0); //Acked
                encoder.table.ref_count.insert(1, 0); //Acked
                encoder.table.ref_count.insert(2, 0); //Acked
                encoder.table.known_received_count = 3;
                encoder_cur = 0;
                qpack_test_cases!(
                    encoder,
                    encoder_buf,
                    encoder_cur,
                    3, "068111",
                    "810d637573746f6d2d76616c756532",
                    272,
                    {
                        Field::Other(String::from("custom-key")),
                        String::from("custom-value2")
                    },
                );
            }

        }
    }
}