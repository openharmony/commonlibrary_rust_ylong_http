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

pub type Result<T> = std::result::Result<T, Err>;

// pub trait TransferData {
//     fn peek_u(&self, ty: &str, len: usize) -> &str;
//     fn get_u(&self, ty: &str, len: usize) -> &str;
//     fn put_u(&self, ty: &str, value: usize,len: usize) -> &mut [u8];
// }
//
// impl TransferData for Octets {
//     fn peek_u(&self, ty: &str, len: usize) -> &str {
//         todo!()
//     }
//
//     fn get_u(&self, ty: &str, len: usize) -> &str {
//         todo!()
//     }
//
//     fn put_u(&self, ty: &str, value: usize, len: usize) -> &mut [u8] {
//         todo!()
//     }
// }
// buf fragment splited by out offset
#[derive(Debug, PartialEq, Eq)]
pub struct ReadVarint<'a> {
    buf: &'a [u8],
}

impl<'a> ReadVarint<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        ReadVarint { buf }
    }

    pub fn into_u8(&mut self) -> Result<u8> {
        const len: usize = 1;

        if self.buf.len() < len {
            return Err(BufferTooShortError);
        }
        let bytes: [u8; len] = <[u8; len]>::try_from(self.buf[..len].as_ref()).unwrap();
        if cfg!(target_endian = "big") {
            let res: u8 = u8::from_be_bytes(bytes);
            Ok(res)
        } else {
            let res: u8 = u8::from_le_bytes(bytes);
            Ok(res)
        }
    }

    pub fn into_u16(&mut self) -> Result<u16> {
        const len: usize = 2;

        if self.buf.len() < len {
            return Err(BufferTooShortError);
        }
        let bytes: [u8; len] = <[u8; len]>::try_from(self.buf[..len].as_ref()).unwrap();
        if cfg!(target_endian = "big") {
            let res: u16 = u16::from_be_bytes(bytes);
            Ok(res)
        } else {
            let res: u16 = u16::from_le_bytes(bytes);
            Ok(res)
        }
    }

    pub fn into_u32(&mut self) -> Result<u32> {
        const len: usize = 4;

        if self.buf.len() < len {
            return Err(BufferTooShortError);
        }
        let bytes: [u8; len] = <[u8; len]>::try_from(self.buf[..len].as_ref()).unwrap();
        if cfg!(target_endian = "big") {
            let res: u32 = u32::from_be_bytes(bytes);
            Ok(res)
        } else {
            let res: u32 = u32::from_le_bytes(bytes);
            Ok(res)
        }
    }

    pub fn into_u64(&mut self) -> Result<u64> {
        const len: usize = 8;

        if self.buf.len() < len {
            return Err(BufferTooShortError);
        }
        let bytes: [u8; len] = <[u8; len]>::try_from(self.buf[..len].as_ref()).unwrap();
        if cfg!(target_endian = "big") {
            let res: u64 = u64::from_be_bytes(bytes);
            Ok(res)
        } else {
            let res: u64 = u64::from_le_bytes(bytes);
            Ok(res)
        }
    }

    /// Reads an unsigned variable-length integer in network byte-order from
    /// the current offset and advances the buffer.
    pub fn get_varint(&mut self) -> Result<u64> {
        let first = self.into_u8()?;

        let len = varint_parse_len(first);

        if len > self.cap() {
            return Err(BufferTooShortError);
        }

        let out = match len {
            1 => u64::from(self.into_u8()?),

            2 => u64::from(self.into_u16()? & 0x3fff),

            4 => u64::from(self.into_u32()? & 0x3fffffff),

            8 => self.into_u64()? & 0x3fffffffffffffff,

            _ => unreachable!(),
        };

        Ok(out)
    }

    /// Returns the remaining capacity in the buffer.
    pub fn cap(&self) -> usize {
        self.buf.len() - self.off
    }
}

// Component encoding status.
enum TokenStatus<T, E> {
    // The current component is completely encoded.
    Complete(T),
    // The current component is partially encoded.
    Partial(E),
}

type TokenResult<T> = Result<TokenStatus<usize, T>>;

struct WriteData<'a> {
    src: &'a [u8],
    src_idx: &'a mut usize,
    dst: &'a mut [u8],
}

impl<'a> WriteData<'a> {
    fn new(src: &'a [u8], src_idx: &'a mut usize, dst: &'a mut [u8]) -> Self {
        WriteData { src, src_idx, dst }
    }

    fn write(&mut self) -> TokenResult<usize> {
        let src_idx = *self.src_idx;
        let input_len = self.src.len() - src_idx;
        let output_len = self.dst.len();
        let num = (&self.src[src_idx..]).read(self.dst).unwrap();
        if output_len >= input_len {
            return Ok(TokenStatus::Complete(num));
        }
        *self.src_idx += num;
        Ok(TokenStatus::Partial(num))
    }
}

pub struct WriteVarint<'a> {
    src: &'a [u8],
    src_idx: &'a mut usize,
    dst: &'a mut [u8],
}

impl<'a> WriteVarint<'a> {
    // pub fn new(buf: &'a mut [u8]) -> Self {
    //     WriteVarint { buf }
    // }
    pub fn new(src: &'a [u8], src_idx: &'a mut usize, dst: &'a mut [u8]) -> Self {
        // src需要从value转码过来
        WriteVarint { src, src_idx, dst }
    }

    /// Writes an unsigned 8-bit integer at the current offset and advances
    /// the buffer.
    pub fn write_u8(&mut self, value: u8) -> Result<usize> {
        const len: usize = 1;
        // buf长度不够问题返回err，再由外层处理
        if self.buf.len() != len {
            return Err(BufferTooShortError);
        }

        let bytes: [u8; len] = value.to_be_bytes();
        self.buf.copy_from_slice(bytes.as_slice());
        Ok(len)
    }

    pub fn write_u16(&mut self, value: u16) -> Result<usize> {
        const len: usize = 2;
        // buf长度不够问题返回err，再由外层处理
        if self.buf.len() != len {
            return Err(BufferTooShortError);
        }

        let bytes: [u8; len] = value.to_be_bytes();
        self.buf.copy_from_slice(bytes.as_slice());
        Ok(len)
    }

    pub fn write_u32(&mut self, value: u32) -> Result<usize> {
        const len: usize = 4;
        // buf长度不够问题返回err，再由外层处理
        if self.buf.len() != len {
            return Err(BufferTooShortError);
        }

        let bytes: [u8; len] = value.to_be_bytes();
        self.buf.copy_from_slice(bytes.as_slice());
        Ok(len)
    }

    pub fn write_u64(&mut self, value: u64) -> Result<usize> {
        const len: usize = 8;
        // buf长度不够问题返回err，再由外层处理
        if self.buf.len() != len {
            return Err(BufferTooShortError);
        }

        let bytes: [u8; len] = value.to_be_bytes();
        self.buf.copy_from_slice(bytes.as_slice());
        Ok(len)
    }

    /// Writes an unsigned variable-length integer in network byte-order at the
    /// current offset and advances the buffer.
    pub fn write_varint(&mut self, value: u64) -> Result<usize> {
        self.write_varint_with_len(value, varint_len(value))
    }

    pub fn write_varint_with_len(&mut self, value: u64, len: usize) -> Result<usize> {
        if self.cap() < len {
            return Err(BufferTooShortError);
        }

        let res = match len {
            1 => self.write_u8(value as u8)?,

            2 => {
                let size = self.write_u16(value as u16)?;
                *self.buf[0] |= 0x40;
                size
            }

            4 => {
                let size = self.write_u32(value as u32)?;
                *self.buf[0] |= 0x80;
                size
            }

            8 => {
                let size = self.write_u64(value)?;
                *self.buf[0] |= 0xc0;
                size
            }

            _ => panic!("value is too large for varint"),
        };

        Ok(res)
    }
}

/// Returns how many bytes it would take to encode `v` as a variable-length
/// integer.
pub const fn varint_len(v: u64) -> usize {
    match v {
        0..=63 => {
            1
        }
        64..=16383 => {
            2
        }
        16384..=1_073_741_823 => {
            4
        }
        1_073_741_824..=4_611_686_018_427_387_903 => {
            8
        }
        _ => {unreachable!()}
    }
}

/// Returns how long the variable-length integer is, given its first byte.
pub const fn varint_parse_len(byte: u8) -> usize {
    let byte = byte >> 6;
    if byte <= 3 {
        1 << byte
    } else {
        unreachable!()
    }
}
