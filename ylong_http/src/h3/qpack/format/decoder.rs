use std::cmp::Ordering;
use crate::h3::error::{ErrorCode, H3Error};
use crate::h3::error::ErrorCode::QPACK_DECOMPRESSION_FAILED;
use crate::h3::qpack::{MidBit, Representation, ReprPrefixBit, RequireInsertCount, DeltaBase, PrefixMask, EncoderInstruction, EncoderInstPrefixBit};
use crate::h3::qpack::integer::IntegerDecoder;
use crate::h3::qpack::format::decoder::DecResult::Error;
use crate::huffman::HuffmanDecoder;

pub(crate) struct EncInstDecoder<'a>{
    buf: &'a [u8],
    state: Option<InstDecodeState>,
}

impl<'a> EncInstDecoder<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, state: None }
    }
    pub(crate) fn load(&mut self, holder: &mut InstDecStateHolder) {
        self.state = holder.state.take();
    }
    pub(crate) fn decode(&mut self) -> Result<Option<EncoderInstruction>, H3Error> {
        if self.buf.is_empty() {
            return Ok(None);
        }

        match self
            .state
            .take()
            .unwrap_or_else(|| InstDecodeState::EncInstIndex(EncInstIndex::new()))
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

pub(crate) struct ReprDecoder<'a> {
    /// `buf` represents the byte stream to be decoded.
    buf: &'a [u8],
    /// `state` represents the remaining state after the last call to `decode`.
    state: Option<ReprDecodeState>,
}

impl<'a> ReprDecoder<'a> {
    /// Creates a new `ReprDecoder` whose `state` is `None`.
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, state: None }
    }

    /// Loads state from a holder.
    pub(crate) fn load(&mut self, holder: &mut ReprDecStateHolder) {
        self.state = holder.state.take();
    }

    /// Decodes `self.buf`. Every time users call `decode`, it will try to
    /// decode a `Representation`.
    pub(crate) fn decode(&mut self) -> Result<Option<Representation>, H3Error> {
        // If buf is empty, leave the state unchanged.
        if self.buf.is_empty() {
            return Ok(None);
        }

        match self
            .state
            .take()
            .unwrap_or_else(|| ReprDecodeState::FiledSectionPrefix(FiledSectionPrefix::new()))
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
                self.state = Some(ReprDecodeState::ReprIndex(ReprIndex::new()));
                Ok(Some(repr))
            }

            DecResult::Error(error) => Err(error),
        }
    }
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
pub(crate) struct ReprDecStateHolder {
    state: Option<ReprDecodeState>,
}

impl ReprDecStateHolder {
    pub(crate) fn new() -> Self {
        Self { state: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.state.is_none()
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
    EncoderInstruction,
    EncInstIndex,
    InstValueString,
    InstNameAndValue,
);

state_def!(
    ReprDecodeState,
    Representation,
    FiledSectionPrefix,
    ReprIndex,
    ReprValueString,
    ReprNameAndValue,
);
state_def!(InstIndexInner, (EncoderInstPrefixBit, MidBit, usize), InstFirstByte, InstTrailingBytes);
state_def!(ReprIndexInner, (ReprPrefixBit, MidBit, usize), ReprFirstByte, ReprTrailingBytes);
state_def!(FSPInner, (RequireInsertCount, bool, DeltaBase), FSPTwoIntergers);
state_def!(
    LiteralString,
    Vec<u8>,
    LengthFirstByte,
    LengthTrailingBytes,
    AsciiStringBytes,
    HuffmanStringBytes,
);

pub(crate) struct FiledSectionPrefix {
    inner: FSPInner,
}

impl FiledSectionPrefix {
    fn new() -> Self {
        Self::from_inner(FSPTwoIntergers.into())
    }
    fn from_inner(inner: FSPInner) -> Self {
        Self { inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((ric, signal, delta_base)) => {
                DecResult::Decoded(Representation::FieldSectionPrefix {
                    require_insert_count: ric,
                    signal: signal,
                    delta_base: delta_base,
                })
            }
            DecResult::NeedMore(inner) => DecResult::NeedMore(FiledSectionPrefix::from_inner(inner).into()),
            DecResult::Error(e) => e.into(),
        }
    }
}

pub(crate) struct EncInstIndex {
    inner: InstIndexInner
}

impl EncInstIndex {
    fn new() -> Self {
        Self::from_inner(InstFirstByte.into())
    }
    fn from_inner(inner: InstIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<EncoderInstruction, InstDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((EncoderInstPrefixBit::SETCAP, _, index)) => {
                DecResult::Decoded(EncoderInstruction::SetCap { capacity:index })
            }
            DecResult::Decoded((EncoderInstPrefixBit::INSERTWITHINDEX, mid_bit, index)) => {
                InstValueString::new(EncoderInstPrefixBit::INSERTWITHINDEX, mid_bit, Name::Index(index)).decode(buf)
            }
            DecResult::Decoded((EncoderInstPrefixBit::INSERTWITHLITERAL, mid_bit, namelen)) => {
                InstNameAndValue::new(EncoderInstPrefixBit::INSERTWITHLITERAL, mid_bit, namelen).decode(buf)
            }
            DecResult::Decoded((EncoderInstPrefixBit::DUPLICATE, _, index)) => {
                DecResult::Decoded(EncoderInstruction::Duplicate { index })
            }
            DecResult::NeedMore(inner) => DecResult::NeedMore(EncInstIndex::from_inner(inner).into()),
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3Error::ConnectionError(ErrorCode::QPACK_DECOMPRESSION_FAILED)),
        }
    }
}

pub(crate) struct ReprIndex {
    inner: ReprIndexInner,
}

impl ReprIndex {
    fn new() -> Self {
        Self::from_inner(ReprFirstByte.into())
    }
    fn from_inner(inner: ReprIndexInner) -> Self {
        Self { inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded((ReprPrefixBit::INDEXED, mid_bit, index)) => {
                DecResult::Decoded(Representation::Indexed { mid_bit, index })
            }
            DecResult::Decoded((ReprPrefixBit::INDEXEDWITHPOSTINDEX, _, index)) => {
                DecResult::Decoded(Representation::IndexedWithPostIndex { index })
            }
            DecResult::Decoded((ReprPrefixBit::LITERALWITHINDEXING, mid_bit, index)) => {
                ReprValueString::new(ReprPrefixBit::LITERALWITHINDEXING, mid_bit, Name::Index(index)).decode(buf)
            }
            DecResult::Decoded((ReprPrefixBit::LITERALWITHPOSTINDEXING, mid_bit, index)) => {
                ReprValueString::new(ReprPrefixBit::LITERALWITHPOSTINDEXING, mid_bit, Name::Index(index)).decode(buf)
            }
            DecResult::Decoded((ReprPrefixBit::LITERALWITHLITERALNAME, mid_bit, namelen)) => {
                ReprNameAndValue::new(ReprPrefixBit::LITERALWITHLITERALNAME, mid_bit, namelen).decode(buf)
            }
            DecResult::NeedMore(inner) => DecResult::NeedMore(ReprIndex::from_inner(inner).into()),
            DecResult::Error(e) => e.into(),
            _ => DecResult::Error(H3Error::ConnectionError(ErrorCode::QPACK_DECOMPRESSION_FAILED )),
        }
    }
}

pub(crate) struct FSPTwoIntergers;

impl FSPTwoIntergers {
    fn decode(self, buf: &mut &[u8]) -> DecResult<(RequireInsertCount, bool, DeltaBase), FSPInner> {
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let mask = PrefixMask::REQUIREINSERTCOUNT;
        *buf = &buf[1..];
        let ric = match IntegerDecoder::first_byte(byte, mask.0) {
            Ok(ric) => ric,
            Err(mut int) => {
                let mut res: usize;
                loop {
                    // If `buf` has been completely decoded here, return the current state.
                    if buf.is_empty() {
                        return DecResult::NeedMore(self.into());
                    }

                    let byte = buf[0];
                    *buf = &buf[1..];
                    // Updates trailing bytes until we get the index.
                    match int.next_byte(byte) {
                        Ok(None) => {}
                        Ok(Some(index)) => {
                            res = index;
                            break;
                        }
                        Err(e) => return e.into(),
                    }
                };
                res
            }
        };
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let signal = (byte & 0x80) != 0;
        let mask = PrefixMask::DELTABASE;
        *buf = &buf[1..];
        let delta_base = match IntegerDecoder::first_byte(byte, mask.0) {
            Ok(delta_base) => delta_base,
            Err(mut int) => {
                let mut res: usize;
                loop {
                    // If `buf` has been completely decoded here, return the current state.
                    if buf.is_empty() {
                        return DecResult::NeedMore(self.into());
                    }

                    let byte = buf[0];
                    *buf = &buf[1..];
                    // Updates trailing bytes until we get the index.
                    match int.next_byte(byte) {
                        Ok(None) => {}
                        Ok(Some(index)) => {
                            res = index;
                            break;
                        }
                        Err(e) => return e.into(),
                    }
                };
                res
            }
        };
        DecResult::Decoded((RequireInsertCount(ric), signal, DeltaBase(delta_base)))
    }
}
pub(crate) struct InstFirstByte;

impl InstFirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<(EncoderInstPrefixBit, MidBit, usize), InstIndexInner> {
        // If `buf` has been completely decoded here, return the current state.
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let inst = EncoderInstPrefixBit::from_u8(byte);
        let mid_bit = inst.prefix_midbit_value(byte);
        let mask = inst.prefix_index_mask();

        // Moves the pointer of `buf` backward.
        *buf = &buf[1..];
        match IntegerDecoder::first_byte(byte, mask.0) {
            // Return the ReprPrefixBit and index part value.
            Ok(idx) => DecResult::Decoded((inst, mid_bit, idx)),
            // Index part value is longer than index(i.e. use all 1 to represent), so it needs more bytes to decode.
            Err(int) => InstTrailingBytes::new(inst, mid_bit, int).decode(buf),
        }
    }
}

pub(crate) struct ReprFirstByte;

impl ReprFirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<(ReprPrefixBit, MidBit, usize), ReprIndexInner> {
        // If `buf` has been completely decoded here, return the current state.
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }
        let byte = buf[0];
        let repr = ReprPrefixBit::from_u8(byte);
        let mid_bit = repr.prefix_midbit_value(byte);
        let mask = repr.prefix_index_mask();

        // Moves the pointer of `buf` backward.
        *buf = &buf[1..];
        match IntegerDecoder::first_byte(byte, mask.0) {
            // Return the ReprPrefixBit and index part value.
            Ok(idx) => DecResult::Decoded((repr, mid_bit, idx)),
            // Index part value is longer than index(i.e. use all 1 to represent), so it needs more bytes to decode.
            Err(int) => ReprTrailingBytes::new(repr, mid_bit, int).decode(buf),
        }
    }
}
pub(crate) struct InstTrailingBytes {
    inst: EncoderInstPrefixBit,
    mid_bit: MidBit,
    index: IntegerDecoder,
}

impl InstTrailingBytes {
    fn new(inst: EncoderInstPrefixBit, mid_bit: MidBit, index: IntegerDecoder) -> Self {
        Self { inst, mid_bit, index }
    }
    fn decode(mut self, buf: &mut &[u8]) -> DecResult<(EncoderInstPrefixBit, MidBit, usize), InstIndexInner> {
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
                Ok(Some(index)) => return DecResult::Decoded((self.inst, self.mid_bit, index)),
                Err(e) => return e.into(),
            }
        }
    }
}
pub(crate) struct ReprTrailingBytes {
    repr: ReprPrefixBit,
    mid_bit: MidBit,
    index: IntegerDecoder,
}

impl ReprTrailingBytes {
    fn new(repr: ReprPrefixBit, mid_bit: MidBit, index: IntegerDecoder) -> Self {
        Self { repr, mid_bit, index }
    }
    fn decode(mut self, buf: &mut &[u8]) -> DecResult<(ReprPrefixBit, MidBit, usize), ReprIndexInner> {
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
                Ok(Some(index)) => return DecResult::Decoded((self.repr, self.mid_bit, index)),
                Err(e) => return e.into(),
            }
        }
    }
}

pub(crate) struct LengthTrailingBytes {
    is_huffman: bool,
    length: IntegerDecoder,
}

impl LengthTrailingBytes {
    fn new(is_huffman: bool, length: IntegerDecoder) -> Self {
        Self { is_huffman, length }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        loop {
            if buf.is_empty() {
                return DecResult::NeedMore(self.into());
            }

            let byte = buf[0];
            *buf = &buf[1..];
            match (self.length.next_byte(byte), self.is_huffman) {
                (Ok(None), _) => {}
                (Err(e), _) => return e.into(),
                (Ok(Some(length)), true) => return HuffmanStringBytes::new(length).decode(buf),
                (Ok(Some(length)), false) => return AsciiStringBytes::new(length).decode(buf),
            }
        }
    }
}

pub(crate) struct AsciiStringBytes {
    octets: Vec<u8>,
    length: usize,
}

impl AsciiStringBytes {
    fn new(length: usize) -> Self {
        Self {
            octets: Vec::new(),
            length,
        }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        match (buf.len() + self.octets.len()).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.octets.len();
                self.octets.extend_from_slice(&buf[..pos]);
                *buf = &buf[pos..];
                DecResult::Decoded(self.octets)
            }
            Ordering::Less => {
                self.octets.extend_from_slice(buf);
                *buf = &buf[buf.len()..];
                DecResult::NeedMore(self.into())
            }
        }
    }
}
pub(crate) struct InstNameAndValue {
    inst: EncoderInstPrefixBit,
    mid_bit: MidBit,
    inner: LiteralString,
}

impl InstNameAndValue {
    fn new(inst: EncoderInstPrefixBit, mid_bit: MidBit, namelen: usize) -> Self {
        Self::from_inner(inst, mid_bit, AsciiStringBytes::new(namelen).into())
    }
    fn from_inner(inst: EncoderInstPrefixBit, mid_bit: MidBit, inner: LiteralString) -> Self {
        Self { inst, mid_bit, inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<EncoderInstruction, InstDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded(octets) => {
                InstValueString::new(self.inst, self.mid_bit, Name::Literal(octets)).decode(buf)
            }
            DecResult::NeedMore(inner) => {
                DecResult::NeedMore(Self::from_inner(self.inst, self.mid_bit, inner).into())
            }
            DecResult::Error(e) => e.into(),
        }
    }
}
pub(crate) struct ReprNameAndValue {
    repr: ReprPrefixBit,
    mid_bit: MidBit,
    inner: LiteralString,
}

impl ReprNameAndValue {
    fn new(repr: ReprPrefixBit, mid_bit: MidBit, namelen: usize) -> Self {
        Self::from_inner(repr, mid_bit, AsciiStringBytes::new(namelen).into())
    }
    fn from_inner(repr: ReprPrefixBit, mid_bit: MidBit, inner: LiteralString) -> Self {
        Self { repr, mid_bit, inner }
    }
    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match self.inner.decode(buf) {
            DecResult::Decoded(octets) => {
                ReprValueString::new(self.repr, self.mid_bit, Name::Literal(octets)).decode(buf)
            }
            DecResult::NeedMore(inner) => {
                DecResult::NeedMore(Self::from_inner(self.repr, self.mid_bit, inner).into())
            }
            DecResult::Error(e) => e.into(),
        }
    }
}

pub(crate) struct LengthFirstByte;

impl LengthFirstByte {
    fn decode(self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        if buf.is_empty() {
            return DecResult::NeedMore(self.into());
        }

        let byte = buf[0];
        *buf = &buf[1..];
        match (IntegerDecoder::first_byte(byte, 0x7f), (byte & 0x80) == 0x80) {
            (Ok(len), true) => HuffmanStringBytes::new(len).decode(buf),
            (Ok(len), false) => AsciiStringBytes::new(len).decode(buf),
            (Err(int), huffman) => LengthTrailingBytes::new(huffman, int).decode(buf),
        }
    }
}

pub(crate) struct HuffmanStringBytes {
    huffman: HuffmanDecoder,
    read: usize,
    length: usize,
}

impl HuffmanStringBytes {
    fn new(length: usize) -> Self {
        Self {
            huffman: HuffmanDecoder::new(),
            read: 0,
            length,
        }
    }

    fn decode(mut self, buf: &mut &[u8]) -> DecResult<Vec<u8>, LiteralString> {
        match (buf.len() + self.read).cmp(&self.length) {
            Ordering::Greater | Ordering::Equal => {
                let pos = self.length - self.read;
                if self.huffman.decode(&buf[..pos]).is_err() {
                    return H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED).into();
                }
                *buf = &buf[pos..];
                match self.huffman.finish() {
                    Ok(vec) => DecResult::Decoded(vec),
                    Err(_) => H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED).into(),
                }
            }
            Ordering::Less => {
                if self.huffman.decode(buf).is_err() {
                    return H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED).into();
                }
                self.read += buf.len();
                *buf = &buf[buf.len()..];
                DecResult::NeedMore(self.into())
            }
        }
    }
}
#[derive(Clone)]
pub(crate) enum Name {
    Index(usize),
    Literal(Vec<u8>),
}
pub(crate) struct InstValueString {
    inst: EncoderInstPrefixBit,
    mid_bit: MidBit,
    name: Name,
    inner: LiteralString,
}

impl InstValueString {
    fn new(inst: EncoderInstPrefixBit, mid_bit: MidBit, name: Name) -> Self {
        Self::from_inner(inst, mid_bit, name, LengthFirstByte.into())
    }

    fn from_inner(inst: EncoderInstPrefixBit, mid_bit: MidBit, name: Name, inner: LiteralString) -> Self {
        Self { inst, mid_bit, name, inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<EncoderInstruction, InstDecodeState> {
        match (self.inst, self.inner.decode(buf)) {
            (EncoderInstPrefixBit::INSERTWITHINDEX, DecResult::Decoded(value)) => {
                DecResult::Decoded(EncoderInstruction::InsertWithIndex {
                    mid_bit: self.mid_bit,
                    name: self.name,
                    value,
                })
            }
            (EncoderInstPrefixBit::INSERTWITHLITERAL, DecResult::Decoded(value)) => {
                DecResult::Decoded(EncoderInstruction::InsertWithLiteral {
                    mid_bit: self.mid_bit,
                    name: self.name,
                    value,
                })
            }
            (_, _) => Error(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))
        }
    }
}
pub(crate) struct ReprValueString {
    repr: ReprPrefixBit,
    mid_bit: MidBit,
    name: Name,
    inner: LiteralString,
}

impl ReprValueString {
    fn new(repr: ReprPrefixBit, mid_bit: MidBit, name: Name) -> Self {
        Self::from_inner(repr, mid_bit, name, LengthFirstByte.into())
    }

    fn from_inner(repr: ReprPrefixBit, mid_bit: MidBit, name: Name, inner: LiteralString) -> Self {
        Self { repr, mid_bit, name, inner }
    }

    fn decode(self, buf: &mut &[u8]) -> DecResult<Representation, ReprDecodeState> {
        match (self.repr, self.inner.decode(buf)) {
            (ReprPrefixBit::LITERALWITHINDEXING, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralWithIndexing {
                    mid_bit: self.mid_bit,
                    name: self.name,
                    value,
                })
            }
            (ReprPrefixBit::LITERALWITHPOSTINDEXING, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralWithPostIndexing {
                    mid_bit: self.mid_bit,
                    name: self.name,
                    value,
                })
            }
            (ReprPrefixBit::LITERALWITHLITERALNAME, DecResult::Decoded(value)) => {
                DecResult::Decoded(Representation::LiteralWithLiteralName {
                    mid_bit: self.mid_bit,
                    name: self.name,
                    value,
                })
            }
            (_, _) => Error(H3Error::ConnectionError(QPACK_DECOMPRESSION_FAILED))
        }
    }
}


/// Decoder's possible returns during the decoding process.
pub(crate) enum DecResult<D, S> {
    /// Decoder has got a `D`. Users can continue to call `encode` to try to
    /// get the next `D`.
    Decoded(D),

    /// Decoder needs more bytes to decode to get a `D`. Returns the current
    /// decoding state `S`.
    NeedMore(S),

    /// Errors that may occur when decoding.
    Error(H3Error),
}

impl<D, S> From<H3Error> for DecResult<D, S> {
    fn from(e: H3Error) -> Self {
        DecResult::Error(e)
    }
}