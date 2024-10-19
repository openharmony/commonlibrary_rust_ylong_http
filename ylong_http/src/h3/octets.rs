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

use std::convert::TryFrom;
use std::io::Read;

use ylong_runtime::iter::parallel::ParSplit;

use crate::h3::error::CommonError::BufferTooShort;
use crate::h3::error::{EncodeError, H3Error};

pub type Result<T> = std::result::Result<T, H3Error>;

macro_rules! peek_bytes {
    ($buf: expr, $ty: ty, $len: expr) => {{
        if $buf.len() < $len {
            return Err(H3Error::Serialize(BufferTooShort));
        }
        let bytes: [u8; $len] = <[u8; $len]>::try_from($buf[..$len].as_ref()).unwrap();
        let res = <$ty>::from_be_bytes(bytes);
        Ok(res)
    }};
}

macro_rules! poll_bytes {
    ($this: expr, $ty: ty, $len: expr) => {{
        let res = peek_bytes!($this.buf[$this.idx..], $ty, $len);
        $this.idx += $len;
        res
    }};
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReadableBytes<'a> {
    buf: &'a [u8],
    idx: usize,
}

impl<'a> ReadableBytes<'a> {
    pub fn from(buf: &'a [u8]) -> Self {
        ReadableBytes { buf, idx: 0 }
    }

    pub(crate) fn peek_u8(&mut self) -> Result<u8> {
        peek_bytes!(self.buf[self.idx..], u8, 1)
    }

    pub fn poll_u8(&mut self) -> Result<u8> {
        poll_bytes!(self, u8, 1)
    }

    pub fn poll_u16(&mut self) -> Result<u16> {
        poll_bytes!(self, u16, 2)
    }

    pub fn poll_u32(&mut self) -> Result<u32> {
        poll_bytes!(self, u32, 4)
    }

    pub fn poll_u64(&mut self) -> Result<u64> {
        poll_bytes!(self, u64, 8)
    }

    /// Reads an unsigned variable-length integer in network byte-order from
    /// the current offset and advances the buffer.
    pub fn get_varint(&mut self) -> Result<u64> {
        let first = self.peek_u8()?;
        let len = parse_varint_len(first);
        if len > self.cap() {
            return Err(BufferTooShort.into());
        }
        let out = match len {
            1 => u64::from(self.poll_u8()?),

            2 => u64::from(self.poll_u16()? & 0x3fff),

            4 => u64::from(self.poll_u32()? & 0x3fffffff),

            8 => self.poll_u64()? & 0x3fffffffffffffff,

            _ => unreachable!(),
        };

        Ok(out)
    }

    /// Returns the remaining capacity in the buffer.
    pub fn cap(&self) -> usize {
        self.buf.len() - self.idx
    }

    pub fn index(&self) -> usize {
        self.idx
    }

    pub fn remaining(&self) -> &[u8] {
        &self.buf[self.idx..]
    }

    pub fn slice(&mut self, length: usize) -> Result<&[u8]> {
        if self.cap() < length {
            Err(BufferTooShort.into())
        } else {
            let curr = self.idx;
            self.idx += length;
            Ok(&self.buf[curr..self.idx])
        }
    }
}

macro_rules! write_bytes {
    ($this: expr, $value: expr, $len: expr) => {{
        // buf长度不够问题返回err，再由外层处理
        if $this.remaining() < $len {
            return Err(BufferTooShort.into());
        }

        let mut bytes: [u8; $len] = $value.to_be_bytes();
        match $len {
            1 => {}
            2 => bytes[0] |= 0x40,
            4 => bytes[0] |= 0x80,
            8 => bytes[0] |= 0xC0,
            _ => unreachable!(),
        }
        $this.bytes[$this.idx..$this.idx + $len].copy_from_slice(bytes.as_slice());
        $this.idx_add($len);
        Ok($len)
    }};
}

pub struct WritableBytes<'a> {
    bytes: &'a mut [u8],
    idx: usize,
}

impl<'a> WritableBytes<'a> {
    pub fn from(bytes: &'a mut [u8]) -> WritableBytes<'a> {
        Self { bytes, idx: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.idx
    }

    pub fn index(&self) -> usize {
        self.idx
    }

    pub fn idx_add(&mut self, offset: usize) {
        self.idx += offset
    }

    /// Writes an unsigned 8-bit integer at the current offset and advances
    /// the buffer.
    pub fn write_u8(&mut self, value: u8) -> Result<usize> {
        write_bytes!(self, value, 1)
    }

    pub fn write_u16(&mut self, value: u16) -> Result<usize> {
        write_bytes!(self, value, 2)
    }

    pub fn write_u32(&mut self, value: u32) -> Result<usize> {
        write_bytes!(self, value, 4)
    }

    pub fn write_u64(&mut self, value: u64) -> Result<usize> {
        write_bytes!(self, value, 8)
    }

    /// Writes an unsigned variable-length integer in network byte-order at the
    /// current offset and advances the buffer.
    pub fn write_varint(&mut self, value: u64) -> Result<usize> {
        self.write_varint_with_len(value, varint_len(value))
    }

    fn write_varint_with_len(&mut self, value: u64, len: usize) -> Result<usize> {
        if self.remaining() < len {
            return Err(BufferTooShort.into());
        }
        match len {
            1 => self.write_u8(value as u8),
            2 => self.write_u16(value as u16),
            4 => self.write_u32(value as u32),
            8 => self.write_u64(value),
            _ => panic!("value is too large for varint"),
        }
    }
}

/// Returns how many bytes it would take to encode `v` as a variable-length
/// integer.
pub const fn varint_len(v: u64) -> usize {
    match v {
        0..=63 => 1,
        64..=16383 => 2,
        16384..=1_073_741_823 => 4,
        1_073_741_824..=4_611_686_018_427_387_903 => 8,
        _ => {
            unreachable!()
        }
    }
}

/// Returns how long the variable-length integer is, given its first byte.
pub const fn parse_varint_len(byte: u8) -> usize {
    let pre = byte >> 6;
    if pre <= 3 {
        1 << pre
    } else {
        unreachable!()
    }
}

#[cfg(test)]
mod h3_octets {
    use crate::h3::octets::{ReadableBytes, WritableBytes};

    /// UT test cases for `ReadableBytes::get_varint` .
    ///
    /// # Brief
    /// 1. Creates a buf with variable-length integer encoded bytes.
    /// 2. Creates a `ReadableBytes` with the buf.
    /// 3. Calls the get_varint method to get the value.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_readable_bytes() {
        let bytes = [
            0x3F, 0x7F, 0xFF, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF,
        ];
        let mut readable = ReadableBytes::from(&bytes);
        assert_eq!(readable.get_varint().unwrap(), 63);
        assert_eq!(readable.get_varint().unwrap(), 16383);
        assert_eq!(readable.get_varint().unwrap(), 1_073_741_823);
        assert_eq!(readable.get_varint().unwrap(), 4_611_686_018_427_387_903);
        assert_eq!(readable.index(), 15);
    }

    /// UT test cases for `WritableBytes::write_varint` .
    ///
    /// # Brief
    /// 1. Creates a result buf with variable-length integer encoded bytes.
    /// 2. Creates a `WritableBytes` with empty buf.
    /// 3. Calls the write_varint method to write integers.
    /// 4. Checks whether the result is correct.
    #[test]
    fn ut_writable_bytes() {
        let bytes = [
            0x3F, 0x7F, 0xFF, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF,
        ];
        let mut buf = [0u8; 15];
        let mut writable = WritableBytes::from(&mut buf);
        assert_eq!(writable.write_varint(63).unwrap(), 1);
        assert_eq!(writable.write_varint(16383).unwrap(), 2);
        assert_eq!(writable.write_varint(1_073_741_823).unwrap(), 4);
        assert_eq!(writable.write_varint(4_611_686_018_427_387_903).unwrap(), 8);
        assert_eq!(writable.index(), 15);
        assert_eq!(buf, bytes);
    }
}
