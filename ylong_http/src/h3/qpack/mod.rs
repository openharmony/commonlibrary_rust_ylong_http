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

pub mod decoder;
pub mod encoder;
pub(crate) mod error;
pub mod format;
mod integer;
pub mod table;
use crate::h3::qpack::format::decoder::Name;
pub(crate) use decoder::FiledLines;
pub(crate) use decoder::QpackDecoder;
pub(crate) use encoder::DecoderInst;
pub(crate) use encoder::QpackEncoder;

pub(crate) struct RequireInsertCount(usize);

pub(crate) struct DeltaBase(usize);

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct EncoderInstPrefixBit(u8);

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct DecoderInstPrefixBit(u8);

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct ReprPrefixBit(u8);

/// # Prefix bit:
/// ## Encoder Instructions:
/// SETCAP: 0x20
/// INSERTWITHINDEX: 0x80
/// INSERTWITHLITERAL: 0x40
/// DUPLICATE: 0x00
///
/// ## Decoder Instructions:
/// ACK: 0x80
/// STREAMCANCEL: 0x40
/// INSERTCOUNTINCREMENT: 0x00
///
/// ## Representation:
/// INDEXED: 0x80
/// INDEXEDWITHPOSTINDEX: 0x10
/// LITERALWITHINDEXING: 0x40
/// LITERALWITHPOSTINDEXING: 0x00
/// LITERALWITHLITERALNAME: 0x20

impl DecoderInstPrefixBit {
    pub(crate) const ACK: Self = Self(0x80);
    pub(crate) const STREAMCANCEL: Self = Self(0x40);
    pub(crate) const INSERTCOUNTINCREMENT: Self = Self(0x00);

    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x >= 0x80 => Self::ACK,
            x if x >= 0x40 => Self::STREAMCANCEL,
            _ => Self::INSERTCOUNTINCREMENT,
        }
    }

    pub(crate) fn prefix_index_mask(&self) -> PrefixMask {
        match self.0 {
            0x80 => PrefixMask::ACK,
            0x40 => PrefixMask::STREAMCANCEL,
            _ => PrefixMask::INSERTCOUNTINCREMENT,
        }
    }

    pub(crate) fn prefix_midbit_value(&self) -> MidBit {
        MidBit {
            n: None,
            t: None,
            h: None,
        }
    }
}

impl EncoderInstPrefixBit {
    pub(crate) const SETCAP: Self = Self(0x20);
    pub(crate) const INSERTWITHINDEX: Self = Self(0x80);
    pub(crate) const INSERTWITHLITERAL: Self = Self(0x40);
    pub(crate) const DUPLICATE: Self = Self(0x00);

    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x >= 0x80 => Self::INSERTWITHINDEX,
            x if x >= 0x40 => Self::INSERTWITHLITERAL,
            x if x >= 0x20 => Self::SETCAP,
            _ => Self::DUPLICATE,
        }
    }

    pub(crate) fn prefix_index_mask(&self) -> PrefixMask {
        match self.0 {
            0x80 => PrefixMask::INSERTWITHINDEX,
            0x40 => PrefixMask::INSERTWITHLITERAL,
            0x20 => PrefixMask::SETCAP,
            _ => PrefixMask::DUPLICATE,
        }
    }

    pub(crate) fn prefix_midbit_value(&self, byte: u8) -> MidBit {
        match self.0 {
            0x80 => MidBit {
                n: None,
                t: Some((byte & 0x40) != 0),
                h: None,
            },
            0x40 => MidBit {
                n: None,
                t: None,
                h: Some((byte & 0x20) != 0),
            },
            0x20 => MidBit {
                n: None,
                t: None,
                h: None,
            },
            _ => MidBit {
                n: None,
                t: None,
                h: None,
            },
        }
    }
}

impl ReprPrefixBit {
    pub(crate) const INDEXED: Self = Self(0x80);
    pub(crate) const INDEXEDWITHPOSTINDEX: Self = Self(0x10);
    pub(crate) const LITERALWITHINDEXING: Self = Self(0x40);
    pub(crate) const LITERALWITHPOSTINDEXING: Self = Self(0x00);
    pub(crate) const LITERALWITHLITERALNAME: Self = Self(0x20);

    /// Creates a `PrefixBit` from a byte. The interface will convert the
    /// incoming byte to the most suitable prefix bit.
    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x >= 0x80 => Self::INDEXED,
            x if x >= 0x40 => Self::LITERALWITHINDEXING,
            x if x >= 0x20 => Self::LITERALWITHLITERALNAME,
            x if x >= 0x10 => Self::INDEXEDWITHPOSTINDEX,
            _ => Self::LITERALWITHPOSTINDEXING,
        }
    }

    /// Returns the corresponding `PrefixIndexMask` according to the current
    /// prefix bit.
    pub(crate) fn prefix_index_mask(&self) -> PrefixMask {
        match self.0 {
            0x80 => PrefixMask::INDEXED,
            0x40 => PrefixMask::INDEXINGWITHNAME,
            0x20 => PrefixMask::INDEXINGWITHLITERAL,
            0x10 => PrefixMask::INDEXEDWITHPOSTNAME,
            _ => PrefixMask::INDEXINGWITHPOSTNAME,
        }
    }

    /// Unlike Hpack, QPACK has some special value for the first byte of an integer.
    /// Like T indicating whether the reference is into the static or dynamic table.
    pub(crate) fn prefix_midbit_value(&self, byte: u8) -> MidBit {
        match self.0 {
            0x80 => MidBit {
                n: None,
                t: Some((byte & 0x40) != 0),
                h: None,
            },
            0x40 => MidBit {
                n: Some((byte & 0x20) != 0),
                t: Some((byte & 0x10) != 0),
                h: None,
            },
            0x20 => MidBit {
                n: Some((byte & 0x10) != 0),
                t: None,
                h: Some((byte & 0x08) != 0),
            },
            0x10 => MidBit {
                n: None,
                t: None,
                h: None,
            },
            _ => MidBit {
                n: Some((byte & 0x08) != 0),
                t: None,
                h: None,
            },
        }
    }
}

pub(crate) enum EncoderInstruction {
    SetCap {
        capacity: usize,
    },
    InsertWithIndex {
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    },
    InsertWithLiteral {
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    },
    Duplicate {
        index: usize,
    },
}

pub(crate) enum DecoderInstruction {
    Ack { stream_id: usize },
    StreamCancel { stream_id: usize },
    InsertCountIncrement { increment: usize },
}

pub(crate) enum Representation {
    /// An indexed field line format identifies an entry in the static table or an entry in
    /// the dynamic table with an absolute index less than the value of the Base.
    /// 0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 1 | T |      Index (6+)       |
    /// +---+---+-----------------------+
    /// This format starts with the '1' 1-bit pattern, followed by the 'T' bit, indicating
    /// whether the reference is into the static or dynamic table. The 6-bit prefix integer
    /// (Section 4.1.1) that follows is used to locate the table entry for the field line. When T=1,
    /// the number represents the static table index; when T=0, the number is the relative index of
    /// the entry in the dynamic table.
    FieldSectionPrefix {
        require_insert_count: RequireInsertCount,
        signal: bool,
        delta_base: DeltaBase,
    },

    Indexed {
        mid_bit: MidBit,
        index: usize,
    },
    IndexedWithPostIndex {
        index: usize,
    },
    LiteralWithIndexing {
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    },
    LiteralWithPostIndexing {
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    },
    LiteralWithLiteralName {
        mid_bit: MidBit,
        name: Name,
        value: Vec<u8>,
    },
}

//impl debug for Representation

pub(crate) struct MidBit {
    //'N', indicates whether an intermediary is permitted to add this field line to the dynamic
    // table on subsequent hops.
    n: Option<bool>,
    //'T', indicating whether the reference is into the static or dynamic table.
    t: Option<bool>,
    //'H', indicating whether is represented as a Huffman-encoded.
    h: Option<bool>,
}

pub(crate) struct PrefixMask(u8);

impl PrefixMask {
    pub(crate) const REQUIREINSERTCOUNT: Self = Self(0xff);
    pub(crate) const DELTABASE: Self = Self(0x7f);
    pub(crate) const INDEXED: Self = Self(0x3f);
    pub(crate) const SETCAP: Self = Self(0x1f);
    pub(crate) const INSERTWITHINDEX: Self = Self(0x3f);
    pub(crate) const INSERTWITHLITERAL: Self = Self(0x1f);
    pub(crate) const DUPLICATE: Self = Self(0x1f);

    pub(crate) const ACK: Self = Self(0x7f);
    pub(crate) const STREAMCANCEL: Self = Self(0x3f);
    pub(crate) const INSERTCOUNTINCREMENT: Self = Self(0x3f);

    pub(crate) const INDEXINGWITHNAME: Self = Self(0x0f);
    pub(crate) const INDEXINGWITHPOSTNAME: Self = Self(0x07);
    pub(crate) const INDEXINGWITHLITERAL: Self = Self(0x07);
    pub(crate) const INDEXEDWITHPOSTNAME: Self = Self(0x0f);
}
