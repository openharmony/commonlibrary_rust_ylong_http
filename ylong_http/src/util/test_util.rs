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

//! Convert a str slice to u8 vector.

pub fn decode(str: &str) -> Option<Vec<u8>> {
    if str.len() % 2 != 0 {
        return None;
    }
    let mut vec = Vec::new();
    let mut remained = str;
    while !remained.is_empty() {
        let (left, right) = remained.split_at(2);
        match u8::from_str_radix(left, 16) {
            Ok(num) => vec.push(num),
            Err(_) => return None,
        }
        remained = right;
    }
    Some(vec)
}
