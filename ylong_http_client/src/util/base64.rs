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

//! Base64 simple implementation.

pub(crate) fn encode(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();

    for chunk in input.chunks(3) {
        output.push(BASE64_TABLE[((chunk[0] >> 2) & 0x3f) as usize]);
        if chunk.len() == 3 {
            output.push(BASE64_TABLE[(((chunk[0] & 0x3) << 4) | ((chunk[1] >> 4) & 0xf)) as usize]);
            output.push(BASE64_TABLE[(((chunk[1] & 0xf) << 2) | ((chunk[2] >> 6) & 0x3)) as usize]);
            output.push(BASE64_TABLE[(chunk[2] & 0x3f) as usize]);
        } else if chunk.len() == 2 {
            output.push(BASE64_TABLE[(((chunk[0] & 0x3) << 4) | ((chunk[1] >> 4) & 0xf)) as usize]);
            output.push(BASE64_TABLE[((chunk[1] & 0xf) << 2) as usize]);
            output.push(b'=');
        } else if chunk.len() == 1 {
            output.push(BASE64_TABLE[((chunk[0] & 0x3) << 4) as usize]);
            output.push(b'=');
            output.push(b'=');
        }
    }
    output
}

static BASE64_TABLE: [u8; 64] = [
    // 0     1     2     3     4     5     6     7
    b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', // 0
    b'I', b'J', b'K', b'L', b'M', b'N', b'O', b'P', // 1
    b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', // 2
    b'Y', b'Z', b'a', b'b', b'c', b'd', b'e', b'f', // 3
    b'g', b'h', b'i', b'j', b'k', b'l', b'm', b'n', // 4
    b'o', b'p', b'q', b'r', b's', b't', b'u', b'v', // 5
    b'w', b'x', b'y', b'z', b'0', b'1', b'2', b'3', // 6
    b'4', b'5', b'6', b'7', b'8', b'9', b'+', b'/', // 7
];

#[cfg(test)]
mod ut_util_base64 {
    use crate::util::base64::encode;

    /// UT test cases for `base64::encode`.
    ///
    /// # Brief
    /// 1. Calls `encode` to parse the string and convert it into `base64`
    ///    format.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_util_base64_encode() {
        assert_eq!(encode(b"this is an example"), b"dGhpcyBpcyBhbiBleGFtcGxl");
        assert_eq!(encode(b"hello"), b"aGVsbG8=");
        assert_eq!(encode(b""), b"");
        assert_eq!(encode(b"a"), b"YQ==");
        assert_eq!(encode(b"ab"), b"YWI=");
        assert_eq!(encode(b"abc"), b"YWJj");
    }
}
