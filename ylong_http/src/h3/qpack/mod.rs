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

pub mod decoder;
pub mod encoder;
pub(crate) mod error;
pub mod format;
mod integer;
pub mod table;
pub(crate) use decoder::{FieldDecodeState, FiledLines, QpackDecoder};
pub(crate) use encoder::{DecoderInst, QpackEncoder};

use crate::h3::qpack::format::decoder::Name;

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
/// SET_CAP: 0x20
/// INSERT_WITH_INDEX: 0x80
/// INSERT_WITH_LITERAL: 0x40
/// DUPLICATE: 0x00
///
/// ## Decoder Instructions:
/// ACK: 0x80
/// STREAM_CANCEL: 0x40
/// INSERT_COUNT_INCREMENT: 0x00
///
/// ## Representation:
/// INDEXED: 0x80
/// INDEXEDWITHPOSTINDEX: 0x10
/// LITERALWITHINDEXING: 0x40
/// LITERALWITHPOSTINDEXING: 0x00
/// LITERALWITHLITERALNAME: 0x20

impl DecoderInstPrefixBit {
    pub(crate) const ACK: Self = Self(0x80);
    pub(crate) const STREAM_CANCEL: Self = Self(0x40);
    pub(crate) const INSERT_COUNT_INCREMENT: Self = Self(0x00);

    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x & 0x80 == 0x80 => Self::ACK,
            x if x & 0xC0 == 0x40 => Self::STREAM_CANCEL,
            x if x & 0xC0 == 0x0 => Self::INSERT_COUNT_INCREMENT,
            _ => unreachable!(),
        }
    }

    pub(crate) fn prefix_index_mask(&self) -> PrefixMask {
        match self.0 {
            0x80 => PrefixMask::ACK,
            0x40 => PrefixMask::STREAM_CANCEL,
            0x0 => PrefixMask::INSERT_COUNT_INCREMENT,
            _ => unreachable!(),
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
    pub(crate) const SET_CAP: Self = Self(0x20);
    pub(crate) const INSERT_WITH_INDEX: Self = Self(0x80);
    pub(crate) const INSERT_WITH_LITERAL: Self = Self(0x40);
    pub(crate) const DUPLICATE: Self = Self(0x00);

    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x >= 0x80 => Self::INSERT_WITH_INDEX,
            x if x >= 0x40 => Self::INSERT_WITH_LITERAL,
            x if x >= 0x20 => Self::SET_CAP,
            _ => Self::DUPLICATE,
        }
    }

    pub(crate) fn prefix_index_mask(&self) -> PrefixMask {
        match self.0 {
            0x80 => PrefixMask::INSERT_WITH_INDEX,
            0x40 => PrefixMask::INSERT_WITH_LITERAL,
            0x20 => PrefixMask::SET_CAP,
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
    // 此处的值为前缀1的位置，并没有实际意义
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
            0x40 => PrefixMask::INDEXING_WITH_NAME,
            0x20 => PrefixMask::INDEXING_WITH_LITERAL,
            0x10 => PrefixMask::INDEXED_WITH_POST_NAME,
            _ => PrefixMask::INDEXING_WITH_LITERAL,
        }
    }

    /// Unlike Hpack, QPACK has some special value for the first byte of an
    /// integer. Like T indicating whether the reference is into the static
    /// or dynamic table.
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
    /// An indexed field line format identifies an entry in the static table or
    /// an entry in the dynamic table with an absolute index less than the
    /// value of the Base. 0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 1 | T |      Index (6+)       |
    /// +---+---+-----------------------+
    /// This format starts with the '1' 1-bit pattern, followed by the 'T' bit,
    /// indicating whether the reference is into the static or dynamic
    /// table. The 6-bit prefix integer (Section 4.1.1) that follows is used
    /// to locate the table entry for the field line. When T=1, the number
    /// represents the static table index; when T=0, the number is the relative
    /// index of the entry in the dynamic table.
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

// impl debug for Representation

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
    pub(crate) const REQUIRE_INSERT_COUNT: Self = Self(0xff);
    pub(crate) const DELTA_BASE: Self = Self(0x7f);
    pub(crate) const INDEXED: Self = Self(0x3f);
    pub(crate) const SET_CAP: Self = Self(0x1f);
    pub(crate) const INSERT_WITH_INDEX: Self = Self(0x3f);
    pub(crate) const INSERT_WITH_LITERAL: Self = Self(0x1f);
    pub(crate) const DUPLICATE: Self = Self(0x1f);
    pub(crate) const ACK: Self = Self(0x7f);
    pub(crate) const STREAM_CANCEL: Self = Self(0x3f);
    pub(crate) const INSERT_COUNT_INCREMENT: Self = Self(0x3f);
    pub(crate) const INDEXING_WITH_NAME: Self = Self(0x0f);
    pub(crate) const INDEXING_WITH_POST_NAME: Self = Self(0x07);
    pub(crate) const INDEXING_WITH_LITERAL: Self = Self(0x07);
    pub(crate) const INDEXED_WITH_POST_NAME: Self = Self(0x0f);
}
