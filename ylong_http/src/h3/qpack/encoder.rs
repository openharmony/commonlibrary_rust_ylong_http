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
}

impl QpackEncoder {
    pub(crate) fn with_capacity(max_size: usize, encoder_buf: &mut [u8], stream_id: usize) -> (QpackEncoder, usize) {
        let mut s = Self {
            table: DynamicTable::with_capacity(max_size),
            holder: ReprEncStateHolder::new(),
            inst_holder: InstDecStateHolder::new(),
            stream_reference: VecDeque::new(),
            stream_id,
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
                                        if let Some(count) = self.table.ref_count.get(&ind){
                                            self.table.ref_count.insert(ind, count - 1);
                                        }
                                    }
                                }
                                else {
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
    pub(crate) fn encode(&mut self, encoder_buf: &mut [u8], stream_buf: &mut [u8]) -> (usize, usize) {
        let mut encoder = ReprEncoder::new(&mut self.table);
        encoder.load(&mut self.holder);
        let (cur_encoder, cur_stream) = encoder.encode(&mut encoder_buf[0..], &mut stream_buf[0..], &mut self.stream_reference);
        let mut cur_prefix = 0;
        if self.is_finished() {
            // denote an end of field section
            self.stream_reference.push_back(None);
            let wireRIC = self.table.insert_count % (2 * self.table.max_entries()) + 1;
            let mut prefix_buf = [0u8; 1024];
            cur_prefix = Integer::index(0x00, wireRIC, 0xff).encode(&mut prefix_buf[..]).unwrap_or(0);
            if self.table.known_received_count >= self.table.insert_count {
                cur_prefix = Integer::index(0x00, self.table.known_received_count - self.table.insert_count, 0x7f).encode(&mut prefix_buf[cur_prefix..]).unwrap_or(0);
            } else {
                cur_prefix = Integer::index(0x80, self.table.insert_count - self.table.known_received_count - 1, 0x7f).encode(&mut prefix_buf[cur_prefix..]).unwrap_or(0);
            }
            // add prefix_buf[..cur_prefix] to the front of stream_buf
            stream_buf.to_vec().splice(0..0, prefix_buf[..cur_prefix].to_vec());
        }
        (cur_encoder, cur_stream + cur_prefix)
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
    use crate::h3::qpack::encoder::{QpackEncoder};
    use crate::h3::qpack::table::Field;

    #[test]
    fn test() {
        let mut encoder = QpackEncoder::with_capacity(4096, &mut [0u8; 1024], 0);
    }
}