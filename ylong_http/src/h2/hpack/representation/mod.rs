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

//! [Header Field Representation] implementation of [HPACK].
//!
//! [Header Field Representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-2.4
//! [HPACK]: https://httpwg.org/specs/rfc7541.html
//!
//! # Description from RFC7541
//! An encoded header field can be represented either as an index or as a
//! literal.
//!
//! An [indexed representation] defines a header field as a reference to an
//! entry in either the static table or the dynamic table.
//!
//! A [literal representation] defines a header field by specifying its
//! name and value. The header field name can be represented literally or as a
//! reference to an entry in either the static table or the dynamic table.
//! The header field value is represented literally.
//!
//! Three different literal representations are defined:
//!
//! - A literal representation that adds the header field as a new entry at the
//! beginning of the dynamic table (see
//! [Literal Header Field with Incremental Indexing]).
//!
//! - A literal representation that does not add the header field to the dynamic
//! table (see [Literal Header Field without Indexing]).
//!
//! - A literal representation that does not add the header field to the dynamic
//! table, with the additional stipulation that this header field always use a
//! literal representation, in particular when re-encoded by an intermediary
//! (see [Literal Header Field Never Indexed]. This representation is intended
//! for protecting header field values that are not to be put at risk by
//! compressing them (see [Never-Indexed Literals] for more details).
//!
//! The selection of one of these literal representations can be guided by
//! security considerations, in order to protect sensitive header field values
//! (see [Probing Dynamic Table State]).
//!
//! The literal representation of a header field name or of a header field value
//! can encode the sequence of octets either directly or using a static
//! Huffman code (see [String Literal Representation]).
//!
//! [Literal Header Field Never Indexed]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.3
//! [Literal Header Field with Incremental Indexing]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.1
//! [Literal Header Field without Indexing]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.2
//! [Never-Indexed Literals]: https://www.rfc-editor.org/rfc/rfc7541.html#section-7.1.3
//! [Probing Dynamic Table State]: https://www.rfc-editor.org/rfc/rfc7541.html#section-7.1
//! [String Literal Representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-5.2
//! [indexed representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.1
//! [literal representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2

mod decoder;
mod encoder;

pub(crate) use decoder::{ReprDecStateHolder, ReprDecodeState, ReprDecoder};
pub(crate) use encoder::{ReprEncStateHolder, ReprEncodeState, ReprEncoder};

/// Definition and [binary format] of each of the different
/// [header field representations] and the [dynamic table size update]
/// instruction.
///
/// [binary format]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6
/// [header field representations]: https://www.rfc-editor.org/rfc/rfc7541.html#section-3.2
/// [dynamic table size update]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.3
pub(crate) enum Representation {
    /// An [indexed header field representation] identifies an entry in either
    /// the static table or the dynamic table. It causes a header field to be
    /// added to the decoded header list.
    ///
    /// An indexed header field starts with the '1' 1-bit pattern, followed by
    /// the index of the matching header field, represented as an integer with
    /// a 7-bit prefix.
    ///
    /// [indexed header field representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.1
    ///
    /// # Binary Format
    /// `Indexed Header Field`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 1 |        Index (7+)         |
    /// +---+---------------------------+
    /// ```
    Indexed { index: usize },

    /// A [literal header field with incremental indexing representation]
    /// results in appending a header field to the decoded header list and
    /// inserting it as a new entry into the dynamic table.
    ///
    /// A literal header field with incremental indexing representation starts
    /// with the '01' 2-bit pattern.
    ///
    /// [literal header field with incremental indexing representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.1
    ///
    /// # Binary Format
    /// `Literal Header Field with Incremental Indexing -- Indexed Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 1 |      Index (6+)       |
    /// +---+---+-----------------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    ///
    /// `Literal Header Field with Incremental Indexing -- New Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 1 |           0           |
    /// +---+---+-----------------------+
    /// | H |     Name Length (7+)      |
    /// +---+---------------------------+
    /// |  Name String (Length octets)  |
    /// +---+---------------------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    LiteralWithIndexing { name: Name, value: Vec<u8> },

    /// A [literal header field without indexing representation] results in
    /// appending a header field to the decoded header list without altering
    /// the dynamic table.
    ///
    /// A literal header field without indexing representation starts with the
    /// '0000' 4-bit pattern.
    ///
    /// [literal header field without indexing representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.2
    ///
    /// # Binary Format
    /// `Literal Header Field without Indexing -- Indexed Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 0 | 0 | 0 |  Index (4+)   |
    /// +---+---+---+---+---------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    /// `Literal Header Field without Indexing -- New Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 0 | 0 | 0 |       0       |
    /// +---+---+---+---+---------------+
    /// | H |     Name Length (7+)      |
    /// +---+---------------------------+
    /// |  Name String (Length octets)  |
    /// +---+---------------------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    LiteralWithoutIndexing { name: Name, value: Vec<u8> },

    /// A [literal header field never-indexed representation] results in
    /// appending a header field to the decoded header list without altering
    /// the dynamic table. Intermediaries **MUST** use the same
    /// representation for encoding this header field.
    ///
    /// A literal header field never-indexed representation starts with the
    /// '0001' 4-bit pattern.
    ///
    /// [literal header field never-indexed representation]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.2.3
    ///
    /// # Binary Format
    /// `Literal Header Field Never Indexed -- Indexed Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 0 | 0 | 1 |  Index (4+)   |
    /// +---+---+---+---+---------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    ///
    /// `Literal Header Field Never Indexed -- New Name`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 0 | 0 | 1 |       0       |
    /// +---+---+---+---+---------------+
    /// | H |     Name Length (7+)      |
    /// +---+---------------------------+
    /// |  Name String (Length octets)  |
    /// +---+---------------------------+
    /// | H |     Value Length (7+)     |
    /// +---+---------------------------+
    /// | Value String (Length octets)  |
    /// +-------------------------------+
    /// ```
    LiteralNeverIndexed { name: Name, value: Vec<u8> },

    /// A [dynamic table size update] signals a change to the size of the
    /// dynamic table.
    ///
    /// [dynamic table size update]: https://www.rfc-editor.org/rfc/rfc7541.html#section-6.3
    ///
    /// # Binary Format
    /// `Maximum Dynamic Table Size Change`:
    /// ```text
    ///   0   1   2   3   4   5   6   7
    /// +---+---+---+---+---+---+---+---+
    /// | 0 | 0 | 1 |   Max size (5+)   |
    /// +---+---+---+-------------------+
    /// ```
    SizeUpdate { max_size: usize },
}

impl Representation {
    /// Gets the prefix bit of the `Representation`.
    pub(crate) const fn prefix_bit(&self) -> PrefixBit {
        match self {
            Self::Indexed { .. } => PrefixBit::INDEXED,
            Self::LiteralWithIndexing { .. } => PrefixBit::LITERAL_WITH_INDEXING,
            Self::SizeUpdate { .. } => PrefixBit::SIZE_UPDATE,
            Self::LiteralNeverIndexed { .. } => PrefixBit::LITERAL_NEVER_INDEXED,
            Self::LiteralWithoutIndexing { .. } => PrefixBit::LITERAL_WITHOUT_INDEXING,
        }
    }

    /// Gets the index mask of the `Representation`.
    pub(crate) const fn prefix_index_mask(&self) -> PrefixIndexMask {
        match self {
            Self::Indexed { .. } => PrefixIndexMask::INDEXED,
            Self::LiteralWithIndexing { .. } => PrefixIndexMask::LITERAL_WITH_INDEXING,
            Self::SizeUpdate { .. } => PrefixIndexMask::SIZE_UPDATE,
            Self::LiteralNeverIndexed { .. } => PrefixIndexMask::LITERAL_NEVER_INDEXED,
            Self::LiteralWithoutIndexing { .. } => PrefixIndexMask::LITERAL_WITHOUT_INDEXING,
        }
    }
}

/// Prefix bit of `Representation`. An integer is represented in two
/// parts: a prefix that fills the current octet and an optional list of octets
/// that are used if the integer value does not fit within the prefix.
///
/// # Binary Format
/// ```text
///   0   1   2   3   4   5   6   7
/// +---+---+---+---+---+---+---+---+
/// | PrefixBit |       Value       |
/// +---+---+---+-------------------+
/// ```
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct PrefixBit(u8);

impl PrefixBit {
    pub(crate) const INDEXED: Self = Self(0x80);
    pub(crate) const LITERAL_WITH_INDEXING: Self = Self(0x40);
    pub(crate) const SIZE_UPDATE: Self = Self(0x20);
    pub(crate) const LITERAL_NEVER_INDEXED: Self = Self(0x10);
    pub(crate) const LITERAL_WITHOUT_INDEXING: Self = Self(0x00);

    /// Creates a `PrefixBit` from a byte. The interface will convert the
    /// incoming byte to the most suitable prefix bit.
    pub(crate) fn from_u8(byte: u8) -> Self {
        match byte {
            x if x >= 0x80 => Self::INDEXED,
            x if x >= 0x40 => Self::LITERAL_WITH_INDEXING,
            x if x >= 0x20 => Self::SIZE_UPDATE,
            x if x >= 0x10 => Self::LITERAL_NEVER_INDEXED,
            _ => Self::LITERAL_WITHOUT_INDEXING,
        }
    }

    /// Returns the corresponding `PrefixIndexMask` according to the current
    /// prefix bit.
    pub(crate) fn prefix_index_mask(&self) -> PrefixIndexMask {
        match self.0 {
            0x80 => PrefixIndexMask::INDEXED,
            0x40 => PrefixIndexMask::LITERAL_WITH_INDEXING,
            0x20 => PrefixIndexMask::SIZE_UPDATE,
            0x10 => PrefixIndexMask::LITERAL_NEVER_INDEXED,
            _ => PrefixIndexMask::LITERAL_WITHOUT_INDEXING,
        }
    }
}

/// Prefix index mask of `Representation`.
///
/// # Binary Format
/// ```text
///   0   1   2   3   4   5   6   7
/// +---+---+---+---+---+---+---+---+
/// | PrefixBit |       Value       |
/// +---+---+---+-------------------+
///
/// +---+---+---+---+---+---+---+---+
/// | 0 | 0 | 0 | 1 | 1 | 1 | 1 | 1 |
/// +---+---+---+---+---+---+---+---+
/// |<-      PrefixIndexMask      ->|
/// ```
pub(crate) struct PrefixIndexMask(u8);

impl PrefixIndexMask {
    pub(crate) const INDEXED: Self = Self(0x7f);
    pub(crate) const LITERAL_WITH_INDEXING: Self = Self(0x3f);
    pub(crate) const SIZE_UPDATE: Self = Self(0x1f);
    pub(crate) const LITERAL_NEVER_INDEXED: Self = Self(0x0f);
    pub(crate) const LITERAL_WITHOUT_INDEXING: Self = Self(0x0f);
}

/// Name of `Representation`. It can be represented as string literals or an
/// index.
pub(crate) enum Name {
    Index(usize),
    Literal(Vec<u8>),
}
