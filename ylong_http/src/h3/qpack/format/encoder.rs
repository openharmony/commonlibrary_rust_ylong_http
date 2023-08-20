use std::arch::asm;
use std::cmp::{max, Ordering};
use std::collections::{HashMap, VecDeque};
use std::result;
use crate::h3::error::ErrorCode::QPACK_DECODER_STREAM_ERROR;
use crate::h3::error::H3Error;
use crate::h3::parts::Parts;
use crate::h3::pseudo::PseudoHeaders;
use crate::h3::qpack::integer::{Integer, IntegerDecoder, IntegerEncoder};
use crate::h3::qpack::{DecoderInstPrefixBit, DecoderInstruction, EncoderInstruction, PrefixMask};
use crate::h3::qpack::format::decoder::DecResult;
use crate::h3::qpack::table::{DynamicTable, Field, TableIndex, TableSearcher};
use crate::headers::HeadersIntoIter;

pub(crate) struct ReprEncoder<'a> {
    table: &'a mut DynamicTable,
    iter: Option<PartsIter>,
    state: Option<ReprEncodeState>,
}


impl<'a> ReprEncoder<'a> {
    /// Creates a new, empty `ReprEncoder`.
    pub(crate) fn new(table: &'a mut DynamicTable) -> Self {
        Self {
            table,
            iter: None,
            state: None,

        }
    }

    /// Loads states from a holder.
    pub(crate) fn load(&mut self, holder: &mut ReprEncStateHolder) {
        self.iter = holder.iter.take();
        self.state = holder.state.take();
    }

    /// Saves current state to a holder.
    pub(crate) fn save(self, holder: &mut ReprEncStateHolder) {
        holder.iter = self.iter;
        holder.state = self.state;
    }

    /// Decodes the contents of `self.iter` and `self.state`. The result will be
    /// written to `dst` and the length of the decoded content will be returned.
    pub(crate) fn encode(&mut self, encoder_buffer: &mut [u8], stream_buffer: &mut [u8], stream_reference: &mut VecDeque<Option<usize>>) -> (usize, usize) {
        let mut cur_encoder = 0;
        let mut cur_stream = 0;
        if let Some(mut iter) = self.iter.take() {
            while let Some((h, v)) = iter.next() {
                let searcher = TableSearcher::new(self.table);
                println!("h: {:?}, v: {:?}", h, v);
                let mut stream_result: Result<usize, ReprEncodeState> = Result::Err(ReprEncodeState::Indexed(Indexed::new(0, false)));
                let mut encoder_result: Result<usize, ReprEncodeState> = Result::Err(ReprEncodeState::Indexed(Indexed::new(0, false)));
                let static_index = searcher.find_index_static(&h, &v);
                if static_index != Some(TableIndex::None) {
                    if let Some(TableIndex::Field(index)) = static_index {
                        // Encode as index in static table
                        stream_result = Indexed::new(index, true).encode(&mut stream_buffer[cur_stream..]);
                    }
                } else {
                    let dynamic_index = searcher.find_index_dynamic(&h, &v);
                    let static_name_index = searcher.find_index_name_static(&h, &v);
                    let mut dynamic_name_index = Some(TableIndex::None);
                    if dynamic_index == Some(TableIndex::None) {
                        if static_name_index == Some(TableIndex::None) {
                            dynamic_name_index = searcher.find_index_name_dynamic(&h, &v);
                        }

                        if self.should_index(&h, &v) && self.table.have_enough_space(&h, &v) {
                            encoder_result = match (&static_name_index, &dynamic_name_index) {
                                // insert with name reference in static table
                                (Some(TableIndex::FieldName(index)), _) => {
                                    InsertWithName::new(index.clone(), v.clone().into_bytes(), false, true).encode(&mut encoder_buffer[cur_encoder..])
                                }
                                // insert with name reference in dynamic table
                                (_, Some(TableIndex::FieldName(index))) => {
                                    // convert abs index to rel index
                                    InsertWithName::new(self.table.insert_count - index.clone() - 1, v.clone().into_bytes(), false, false).encode(&mut encoder_buffer[cur_encoder..])
                                }
                                // insert with literal name
                                (_, _) => {
                                    InsertWithLiteral::new(h.clone().into_string().into_bytes(), v.clone().into_bytes(), false).encode(&mut encoder_buffer[cur_encoder..])
                                }
                            };
                            // update dynamic table
                            self.table.insert_count += 1;
                            self.table.update(h.clone(), v.clone());
                        }
                    }
                    if dynamic_index == Some(TableIndex::None) {
                        if dynamic_name_index != Some(TableIndex::None) {
                            //Encode with name reference in dynamic table
                            if let Some(TableIndex::FieldName(index)) = dynamic_name_index {
                                stream_reference.push_back(Some(index));
                                if let Some(count) = self.table.ref_count.get(&index) {
                                    self.table.ref_count.insert(index, count + 1);
                                }
                                // use post-base index
                                if self.table.known_received_count <= index {
                                    stream_result = IndexingWithPostName::new(index - self.table.known_received_count, v.clone().into_bytes(), false, false).encode(&mut stream_buffer[cur_stream..]);
                                } else {
                                    stream_result = IndexingWithName::new(self.table.known_received_count - index - 1, v.clone().into_bytes(), false, false, false).encode(&mut stream_buffer[cur_stream..]);
                                }
                            }

                        } else {
                            // Encode with name reference in static table
                            // or Encode as Literal
                            if static_name_index != Some(TableIndex::None) {
                                if let Some(TableIndex::FieldName(index)) = static_name_index {
                                    stream_result = IndexingWithName::new(index, v.into_bytes(), false, true, false).encode(&mut stream_buffer[cur_stream..]);
                                }
                            } else {
                                stream_result = IndexingWithLiteral::new(h.into_string().into_bytes(), v.into_bytes(), false, false).encode(&mut stream_buffer[cur_stream..]);
                            }
                        }
                    } else {
                        assert!(dynamic_index != Some(TableIndex::None));
                        // Encode with index in dynamic table
                        if let Some(TableIndex::FieldName(index)) = dynamic_name_index {
                            // use post-base index
                            stream_reference.push_back(Some(index));
                            if let Some(count) = self.table.ref_count.get(&index) {
                                self.table.ref_count.insert(index, count + 1);
                            }
                            if self.table.known_received_count <= index{
                                stream_result = IndexedWithPostName::new(index - self.table.known_received_count).encode(&mut stream_buffer[cur_stream..]);
                            }
                            else {
                                stream_result = Indexed::new(self.table.known_received_count - index - 1, false).encode(&mut stream_buffer[cur_stream..]);
                            }
                        }
                    }
                }

                match (encoder_result, stream_result) {
                    (Ok(encoder_size), Ok(stream_size)) => {
                        cur_stream += stream_size;
                        cur_encoder += encoder_size;
                    }
                    (Err(state), Ok(stream_size)) => {
                        cur_stream += stream_size;
                        self.state = Some(state);
                        self.iter = Some(iter);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                    (Ok(encoder_size), Err(state)) => {
                        cur_encoder += encoder_size;
                        self.state = Some(state);
                        self.iter = Some(iter);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                    (Err(_), Err(state)) => {
                        self.state = Some(state);
                        self.iter = Some(iter);
                        return (encoder_buffer.len(), stream_buffer.len());
                    }
                }
            }
        }
        (cur_encoder, cur_stream)
    }
    /// ## 2.1.1.1. Avoiding Prohibited Insertions
    /// To ensure that the encoder is not prevented from adding new entries, the encoder can
    /// avoid referencing entries that are close to eviction. Rather than reference such an
    /// entry, the encoder can emit a Duplicate instruction (Section 4.3.4) and reference
    /// the duplicate instead.
    ///
    /// Determining which entries are too close to eviction to reference is an encoder preference.
    /// One heuristic is to target a fixed amount of available space in the dynamic table:
    /// either unused space or space that can be reclaimed by evicting non-blocking entries.
    /// To achieve this, the encoder can maintain a draining index, which is the smallest
    /// absolute index (Section 3.2.4) in the dynamic table that it will emit a reference for.
    /// As new entries are inserted, the encoder increases the draining index to maintain the
    /// section of the table that it will not reference. If the encoder does not create new
    /// references to entries with an absolute index lower than the draining index, the number
    /// of unacknowledged references to those entries will eventually become zero, allowing
    /// them to be evicted.
    ///
    ///     <-- Newer Entries          Older Entries -->
    /// (Larger Indices)       (Smaller Indices)
    /// +--------+---------------------------------+----------+
    /// | Unused |          Referenceable          | Draining |
    /// | Space  |             Entries             | Entries  |
    /// +--------+---------------------------------+----------+
    /// ^                                 ^          ^
    /// |                                 |          |
    /// Insertion Point                 Draining Index  Dropping
    /// Point
    pub(crate) fn should_index(&self, header: &Field, value: &str) -> bool {
        //todo: add condition to modify the algorithm
        true
    }
}



pub(crate) struct ReprEncStateHolder {
    iter: Option<PartsIter>,
    state: Option<ReprEncodeState>,
}

impl ReprEncStateHolder {
    /// Creates a new, empty `ReprEncStateHolder`.
    pub(crate) fn new() -> Self {
        Self {
            iter: None,
            state: None,
        }
    }

    /// Creates a state based on the `Parts` to be encoded.
    pub(crate) fn set_parts(&mut self, parts: Parts) {
        self.iter = Some(PartsIter::new(parts))
    }

    /// Determines whether `self.iter` and `self.state` are empty. if they are
    /// empty, it means encoding is finished.
    pub(crate) fn is_empty(&self) -> bool {
        self.iter.is_none() && self.state.is_none()
    }
}

pub(crate) enum ReprEncodeState {
    Indexed(Indexed),
    InsertWithName(InsertWithName),
    InsertWithLiteral(InsertWithLiteral),
    IndexingWithName(IndexingWithName),
    IndexingWithPostName(IndexingWithPostName),
    IndexingWithLiteral(IndexingWithLiteral),
    IndexedWithPostName(IndexedWithPostName),
}


pub(crate) struct Indexed {
    index: Integer,
}

impl Indexed {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize, is_static: bool) -> Self {
        if is_static {
            // in static table
            Self { index: Integer::index(0xc0, index, PrefixMask::INDEXED.0) }
        } else {
            // in dynamic table
            Self { index: Integer::index(0x80, index, PrefixMask::INDEXED.0) }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::Indexed(Indexed::from(e)))
    }
}

pub(crate) struct IndexedWithPostName {
    index: Integer,
}

impl IndexedWithPostName {
    fn from(index: Integer) -> Self {
        Self { index }
    }

    fn new(index: usize) -> Self {
        Self { index: Integer::index(0x10, index, PrefixMask::INDEXINGWITHPOSTNAME.0) }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.index
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexedWithPostName(IndexedWithPostName::from(e)))
    }
}


pub(crate) struct InsertWithName {
    inner: IndexAndValue,
}

impl InsertWithName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(index: usize, value: Vec<u8>, is_huffman: bool, is_static: bool) -> Self {
        if is_static {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0xc0, index, PrefixMask::INSERTWITHINDEX.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x80, index, PrefixMask::INSERTWITHINDEX.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithName(InsertWithName::from(e)))
    }
}


pub(crate) struct IndexingWithName {
    inner: IndexAndValue,
}

impl IndexingWithName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(index: usize, value: Vec<u8>, is_huffman: bool, is_static: bool, no_permit: bool) -> Self {
        match (no_permit, is_static) {
            (true, true) => {
                Self {
                    inner: IndexAndValue::new()
                        .set_index(0x70, index, PrefixMask::INDEXINGWITHNAME.0)
                        .set_value(value, is_huffman),
                }
            }
            (true, false) => {
                Self {
                    inner: IndexAndValue::new()
                        .set_index(0x60, index, PrefixMask::INDEXINGWITHNAME.0)
                        .set_value(value, is_huffman),
                }
            }
            (false, true) => {
                Self {
                    inner: IndexAndValue::new()
                        .set_index(0x50, index, PrefixMask::INDEXINGWITHNAME.0)
                        .set_value(value, is_huffman),
                }
            }
            (false, false) => {
                Self {
                    inner: IndexAndValue::new()
                        .set_index(0x40, index, PrefixMask::INDEXINGWITHNAME.0)
                        .set_value(value, is_huffman),
                }
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexingWithName(IndexingWithName::from(e)))
    }
}


pub(crate) struct IndexingWithPostName {
    inner: IndexAndValue,
}

impl IndexingWithPostName {
    fn from(inner: IndexAndValue) -> Self {
        Self { inner }
    }

    fn new(index: usize, value: Vec<u8>, is_huffman: bool, no_permit: bool) -> Self {
        if no_permit {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x08, index, PrefixMask::INDEXINGWITHPOSTNAME.0)
                    .set_value(value, is_huffman),
            }
        } else {
            Self {
                inner: IndexAndValue::new()
                    .set_index(0x00, index, PrefixMask::INDEXINGWITHPOSTNAME.0)
                    .set_value(value, is_huffman),
            }
        }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::IndexingWithPostName(IndexingWithPostName::from(e)))
    }
}


pub(crate) struct IndexingWithLiteral {
    inner: NameAndValue,
}

impl IndexingWithLiteral {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool, no_permit: bool) -> Self {
        match (no_permit, is_huffman) {
            (true, true) => {
                Self {
                    inner: NameAndValue::new()
                        .set_index(0x38, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                        .set_name_and_value(name, value, is_huffman),
                }
            }
            (true, false) => {
                Self {
                    inner: NameAndValue::new()
                        .set_index(0x30, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                        .set_name_and_value(name, value, is_huffman),
                }
            }
            (false, true) => {
                Self {
                    inner: NameAndValue::new()
                        .set_index(0x28, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                        .set_name_and_value(name, value, is_huffman),
                }
            }
            (false, false) => {
                Self {
                    inner: NameAndValue::new()
                        .set_index(0x20, name.len(), PrefixMask::INDEXINGWITHLITERAL.0)
                        .set_name_and_value(name, value, is_huffman),
                }
            }
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithLiteral(InsertWithLiteral::from(e)))
    }
}


pub(crate) struct InsertWithLiteral {
    inner: NameAndValue,
}

impl InsertWithLiteral {
    fn new(name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        if is_huffman {
            Self {
                inner: NameAndValue::new()
                    .set_index(0x60, name.len(), PrefixMask::INSERTWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        } else {
            Self {
                inner: NameAndValue::new()
                    .set_index(0x40, name.len(), PrefixMask::INSERTWITHLITERAL.0)
                    .set_name_and_value(name, value, is_huffman),
            }
        }
    }

    fn from(inner: NameAndValue) -> Self {
        Self { inner }
    }

    fn encode(self, dst: &mut [u8]) -> Result<usize, ReprEncodeState> {
        self.inner
            .encode(dst)
            .map_err(|e| ReprEncodeState::InsertWithLiteral(InsertWithLiteral::from(e)))
    }
}


pub(crate) struct IndexAndValue {
    index: Option<Integer>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}
macro_rules! check_and_encode {
    ($item: expr, $dst: expr, $cur: expr, $self: expr) => {{
        if let Some(i) = $item.take() {
            match i.encode($dst) {
                Ok(len) => $cur += len,
                Err(e) => {
                    $item = Some(e);
                    return Err($self);
                }
            };
        }
    }};
}
impl IndexAndValue {
    fn new() -> Self {
        Self {
            index: None,
            value_length: None,
            value_octets: None,
        }
    }

    fn set_index(mut self, pre: u8, index: usize, mask: u8) -> Self {
        self.index = Some(Integer::index(pre, index, mask));
        self
    }

    fn set_value(mut self, value: Vec<u8>, is_huffman: bool) -> Self {
        self.value_length = Some(Integer::length(value.len(), is_huffman));
        self.value_octets = Some(Octets::new(value));
        self
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;
        check_and_encode!(self.index, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_octets, &mut dst[cur..], cur, self);
        Ok(cur)
    }
}


pub(crate) struct NameAndValue {
    index: Option<Integer>,
    name_length: Option<Integer>,
    name_octets: Option<Octets>,
    value_length: Option<Integer>,
    value_octets: Option<Octets>,
}

impl NameAndValue {
    fn new() -> Self {
        Self {
            index: None,
            name_length: None,
            name_octets: None,
            value_length: None,
            value_octets: None,
        }
    }

    fn set_index(mut self, pre: u8, index: usize, mask: u8) -> Self {
        self.index = Some(Integer::index(pre, index, mask));
        self
    }

    fn set_name_and_value(mut self, name: Vec<u8>, value: Vec<u8>, is_huffman: bool) -> Self {
        self.name_length = Some(Integer::length(name.len(), is_huffman));
        self.name_octets = Some(Octets::new(name));
        self.value_length = Some(Integer::length(value.len(), is_huffman));
        self.value_octets = Some(Octets::new(value));
        self
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;
        check_and_encode!(self.index, &mut dst[cur..], cur, self);
        // check_and_encode!(self.name_length, &mut dst[cur..], cur, self); //no need for qpack cause it in index.
        check_and_encode!(self.name_octets, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_length, &mut dst[cur..], cur, self);
        check_and_encode!(self.value_octets, &mut dst[cur..], cur, self);
        Ok(cur)
    }
}



macro_rules! state_def {
    ($name: ident, $decoded: ty, $($state: ident),* $(,)?) => {
        pub(crate) enum $name {
            $(
                $state($state),
            )*
        }

        impl $name {
            fn decode(self, buf: &mut &[u8]) -> DecResult<$decoded, $name> {
                match self {
                    $(
                        Self::$state(state) => state.decode(buf),
                    )*
                }
            }
        }

        $(
            impl From<$state> for $name {
                fn from(s: $state) -> Self {
                    Self::$state(s)
                }
            }
        )*
    }
}

state_def!(
    InstDecodeState,
    DecoderInstruction,
    DecInstIndex
);
pub(crate) struct DecInstDecoder<'a>{
    buf: &'a [u8],
    state: Option<InstDecodeState>,
}
pub(crate) struct InstDecStateHolder {
    state: Option<InstDecodeState>,
}

impl InstDecStateHolder {
    pub(crate) fn new() -> Self {
        Self { state: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.state.is_none()
    }
}
impl<'a> DecInstDecoder<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, state: None }
    }
    pub(crate) fn load(&mut self, holder: &mut InstDecStateHolder) {
        self.state = holder.state.take();
    }
    pub(crate) fn decode(&mut self) -> Result<Option<DecoderInstruction>, H3Error> {
        if self.buf.is_empty() {
            return Ok(None);
        }

        match self
            .state
            .take()
            .unwrap_or_else(|| InstDecodeState::DecInstIndex(DecInstIndex::new()))
            .decode(&mut self.buf)
        {
            // If `buf` is not enough to continue decoding a complete
            // `Representation`, `Ok(None)` will be returned. Users need to call
            // `save` to save the current state to a `ReprDecStateHolder`.
            DecResult::NeedMore(state) => {
                println!("NeedMore");
                self.state = Some(state);
                Ok(None)
            }
            DecResult::Decoded(repr) => {
                Ok(Some(repr))
            }

            DecResult::Error(error) => Err(error),
        }
    }
}
state_def!(DecInstIndexInner, (DecoderInstPrefixBit, usize), InstFirstByte, InstTrailingBytes);

pub(crate) struct DecInstIndex {
    inner: DecInstIndexInner
}
impl DecInstIndex {
    fn new() -> Self {
        Self::from_inner(InstFirstByte.into())
    }
    fn from_inner(inner: DecInstIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<DecoderInstruction, InstDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((DecoderInstPrefixBit::ACK, index)) => {
                DecResult::Decoded(DecoderInstruction::Ack { stream_id:index })
            }
            DecResult::Decoded((DecoderInstPrefixBit::STREAMCANCEL, index)) => {
                DecResult::Decoded(DecoderInstruction::StreamCancel { stream_id:index })
            }
            DecResult::Decoded((DecoderInstPrefixBit::INSERTCOUNTINCREMENT, index)) => {
                DecResult::Decoded(DecoderInstruction::InsertCountIncrement { increment: index })
            }
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3Error::ConnectionError(QPACK_DECODER_STREAM_ERROR)),
        }
    }
}

pub(crate) struct InstFirstByte;

impl InstFirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<(DecoderInstPrefixBit, usize), DecInstIndexInner> {
        // If `buf` has been completely decoded here, return the current state.
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let inst = DecoderInstPrefixBit::from_u8(byte);
        let mask = inst.prefix_index_mask();

        // Moves the pointer of `buf` backward.
        *buf = &buf[1..];
        match IntegerDecoder::first_byte(byte, mask.0) {
            // Return the ReprPrefixBit and index part value.
            Ok(idx) => DecResult::Decoded((inst, idx)),
            // Index part value is longer than index(i.e. use all 1 to represent), so it needs more bytes to decode.
            Err(int) => InstTrailingBytes::new(inst, int).decode(buf),
        }
    }
}

pub(crate) struct InstTrailingBytes {
    inst: DecoderInstPrefixBit,
    index: IntegerDecoder,
}

impl InstTrailingBytes {
    fn new(inst: DecoderInstPrefixBit, index: IntegerDecoder) -> Self {
        Self { inst, index }
    }
    fn decode(mut self, buf: &mut &[u8]) -> DecResult<(DecoderInstPrefixBit, usize), DecInstIndexInner> {
        loop {
            // If `buf` has been completely decoded here, return the current state.
            if buf.is_empty() {
                return DecResult::NeedMore(self.into());
            }

            let byte = buf[0];
            *buf = &buf[1..];
            // Updates trailing bytes until we get the index.
            match self.index.next_byte(byte) {
                Ok(None) => {}
                Ok(Some(index)) => return DecResult::Decoded((self.inst, index)),
                Err(e) => return e.into(),
            }
        }
    }
}

pub(crate) struct Octets {
    src: Vec<u8>,
    idx: usize,
}

impl Octets {
    fn new(src: Vec<u8>) -> Self {
        Self { src, idx: 0 }
    }

    fn encode(mut self, dst: &mut [u8]) -> Result<usize, Self> {
        let mut cur = 0;

        let input_len = self.src.len() - self.idx;
        let output_len = dst.len();

        if input_len == 0 {
            return Ok(cur);
        }

        match output_len.cmp(&input_len) {
            Ordering::Greater | Ordering::Equal => {
                dst[..input_len].copy_from_slice(&self.src[self.idx..]);
                cur += input_len;
                Ok(cur)
            }
            Ordering::Less => {
                dst[..].copy_from_slice(&self.src[self.idx..self.idx + output_len]);
                self.idx += output_len;
                Err(self)
            }
        }
    }
}

struct PartsIter {
    pseudo: PseudoHeaders,
    map: HeadersIntoIter,
    next_type: PartsIterDirection,
}

/// `PartsIterDirection` is the `PartsIter`'s direction to get the next header.
enum PartsIterDirection {
    Authority,
    Method,
    Path,
    Scheme,
    Status,
    Other,
}

impl PartsIter {
    /// Creates a new `PartsIter` from the given `Parts`.
    fn new(parts: Parts) -> Self {
        Self {
            pseudo: parts.pseudo,
            map: parts.map.into_iter(),
            next_type: PartsIterDirection::Method,
        }
    }

    /// Gets headers in the order of `Method`, `Status`, `Scheme`, `Path`,
    /// `Authority` and `Other`.
    fn next(&mut self) -> Option<(Field, String)> {
        loop {
            match self.next_type {
                PartsIterDirection::Method => match self.pseudo.take_method() {
                    Some(value) => return Some((Field::Method, value)),
                    None => self.next_type = PartsIterDirection::Status,
                },
                PartsIterDirection::Status => match self.pseudo.take_status() {
                    Some(value) => return Some((Field::Status, value)),
                    None => self.next_type = PartsIterDirection::Scheme,
                },
                PartsIterDirection::Scheme => match self.pseudo.take_scheme() {
                    Some(value) => return Some((Field::Scheme, value)),
                    None => self.next_type = PartsIterDirection::Path,
                },
                PartsIterDirection::Path => match self.pseudo.take_path() {
                    Some(value) => return Some((Field::Path, value)),
                    None => self.next_type = PartsIterDirection::Authority,
                },
                PartsIterDirection::Authority => match self.pseudo.take_authority() {
                    Some(value) => return Some((Field::Authority, value)),
                    None => self.next_type = PartsIterDirection::Other,
                },
                PartsIterDirection::Other => {
                    return self
                        .map
                        .next()
                        .map(|(h, v)| (Field::Other(h.to_string()), v.to_str().unwrap()));
                }
            }
        }
    }
}

