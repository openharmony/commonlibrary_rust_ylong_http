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

//! HTTP url [`PercentEncoder`].
//!
//! URI references are used to target requests, indicate redirects, and define
//! relationships.
//!
//! [`PercentEncoder`]: https://url.spec.whatwg.org/#fragment-percent-encode

use std::str;
use std::str::{Bytes, Chars};

use crate::error::HttpError;
use crate::request::uri::{InvalidUri, Uri};

type Utf8Char<'a> = (char, &'a str);

const USERINFO: &[u8; 19] = b" \"#<>?`{}/:;=@[]^|\\";
const FRAGMENT: &[u8; 5] = b" \"<>`";
const PATH: &[u8; 9] = b" \"#<>?`{}";
const QUERY: &[u8; 6] = b" \"#<>\'";

/// HTTP url percent encoding implementation.
///
/// # Examples
///
/// ```
/// use ylong_http::request::uri::PercentEncoder;
///
/// let url = "https://www.example.com/data/测试文件.txt";
/// let encoded = PercentEncoder::parse(url).unwrap();
/// assert_eq!(
///     encoded,
///     "https://www.example.com/data/%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt"
/// );
/// ```
pub struct PercentEncoder {
    normalized: Normalized,
}

impl PercentEncoder {
    /// Percent-coding entry.
    pub fn parse(origin: &str) -> Result<String, HttpError> {
        let mut encoder = Self {
            normalized: Normalized::from_size(origin.len()),
        };
        let bytes = UrlChars {
            remaining: origin.chars(),
        };
        let remaining = encoder.parse_scheme(bytes)?;
        let remaining = encoder.parse_double_slash(remaining)?;
        let remaining = encoder.parse_userinfo(remaining)?;
        let remaining = encoder.parse_authority(remaining)?;
        let remaining = encoder.parse_path(remaining)?;
        encoder.parse_query_and_fragment(remaining)?;
        Ok(encoder.normalized.url())
    }

    fn parse_scheme<'a>(&mut self, mut origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        while let Some(char) = origin.next() {
            match char {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '+' | '-' | '.' => {
                    self.normalized.push(char.to_ascii_lowercase())
                }
                ':' => {
                    self.normalized.push(char);
                    return Ok(origin);
                }
                _ => return Err(InvalidUri::InvalidScheme.into()),
            }
        }
        Err(InvalidUri::InvalidScheme.into())
    }

    fn parse_double_slash<'a>(&mut self, origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        let mut chars = origin.clone();
        let mut count = 0;
        loop {
            let mut tmp_chars = chars.clone();
            if matches!(tmp_chars.next(), Some(c) if matches!(c, '/')) {
                count += 1;
                chars = tmp_chars;
                self.normalized.push('/');
            } else {
                break;
            }
        }
        if count == 2 {
            Ok(chars)
        } else {
            Err(InvalidUri::InvalidScheme.into())
        }
    }

    fn parse_userinfo<'a>(&mut self, mut origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        let mut chars = origin.clone();

        let mut size = 0;
        let mut after_at = None;
        while let Some(ch) = chars.next() {
            match ch {
                '@' => {
                    after_at = Some((size, chars.clone()));
                }
                '/' | '?' | '#' => break,
                _ => {}
            }
            size += 1;
        }

        let (mut info_len, remaining) = match after_at {
            None => {
                return Ok(origin);
            }
            Some((0, remaining)) => {
                if matches!(remaining.clone().next(), Some(c) if matches!(c, '/' | '?' | '#')) {
                    return Err(InvalidUri::UriMissHost.into());
                }
                return Ok(remaining);
            }
            Some(at) => at,
        };

        let mut has_username = false;
        while info_len > 0 {
            info_len -= 1;
            if let Some(ch) = origin.next_u8() {
                if ch.0 == ':' && !has_username {
                    has_username = true;
                    self.normalized.push(':')
                } else {
                    self.normalized.percent_encoding_push(ch, USERINFO);
                }
            }
        }
        self.normalized.push('@');

        Ok(remaining)
    }
    fn parse_authority<'a>(&mut self, mut origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        loop {
            let chars = origin.clone();
            if let Some(c) = origin.next() {
                match c {
                    '/' | '?' | '#' => {
                        origin = chars;
                        break;
                    }
                    _ => {
                        self.normalized.push(c);
                    }
                }
            } else {
                break;
            }
        }
        Ok(origin)
    }

    fn parse_path<'a>(&mut self, mut origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        loop {
            let chars = origin.clone();
            if let Some((ch, u8_str)) = origin.next_u8() {
                match ch {
                    '/' => {
                        self.normalized.push(ch);
                    }
                    '#' | '?' => {
                        origin = chars;
                        break;
                    }
                    _ => {
                        self.normalized.percent_encoding_push((ch, u8_str), PATH);
                    }
                }
            } else {
                break;
            }
        }

        Ok(origin)
    }

    fn parse_query_and_fragment(&mut self, mut origin: UrlChars) -> Result<(), HttpError> {
        let mut remaining = origin.clone();
        match origin.first_valid() {
            None => {}
            Some('?') => {
                self.normalized.push('?');
                let chars = self.parse_query(origin)?;
                remaining = chars;
            }
            Some('#') => {
                self.normalized.push('#');
                remaining = origin;
            }
            _ => {
                return Err(InvalidUri::InvalidFormat.into());
            }
        }
        self.parse_fragment(remaining)
    }

    fn parse_query<'a>(&mut self, mut origin: UrlChars<'a>) -> Result<UrlChars<'a>, HttpError> {
        while let Some((ch, u8_str)) = origin.next_u8() {
            match ch {
                '#' => {
                    self.normalized.push('#');
                    break;
                }
                _ => self.normalized.percent_encoding_push((ch, u8_str), QUERY),
            }
        }

        Ok(origin)
    }

    fn parse_fragment(&mut self, mut origin: UrlChars) -> Result<(), HttpError> {
        while let Some(utf8) = origin.next_u8() {
            self.normalized.percent_encoding_push(utf8, FRAGMENT);
        }
        Ok(())
    }
}

pub(crate) struct Normalized {
    url: String,
}

impl Normalized {
    pub(crate) fn from_size(size: usize) -> Self {
        Self {
            url: String::with_capacity(size),
        }
    }
    pub(crate) fn push(&mut self, ch: char) {
        if !matches!(ch, '\t' | '\r' | '\n') {
            self.url.push(ch);
        }
    }

    pub(crate) fn percent_encoding_push(&mut self, u8_ch: Utf8Char, char_set: &[u8]) {
        let (ch, u8_str) = u8_ch;
        if !matches!(ch, '\t' | '\r' | '\n') {
            let mut start = 0;
            for (index, &byte) in u8_str.as_bytes().iter().enumerate() {
                if should_percent_encoding(byte, char_set) {
                    if start < index {
                        let unencoded =
                            unsafe { str::from_utf8_unchecked(&u8_str.as_bytes()[start..index]) };
                        self.url.push_str(unencoded);
                    }
                    let encoded = percent_hex(byte);
                    self.url.push('%');
                    self.url.push_str(encoded);

                    start = index + 1;
                }
            }

            let ch_len = u8_str.len();
            if start < ch_len {
                let unencoded =
                    unsafe { str::from_utf8_unchecked(&u8_str.as_bytes()[start..ch_len]) };
                self.url.push_str(unencoded);
            }
        }
    }

    pub(crate) fn url(self) -> String {
        self.url
    }
}

#[derive(Clone)]
struct UrlChars<'a> {
    remaining: Chars<'a>,
}

impl<'a> UrlChars<'a> {
    pub(crate) fn next_u8(&mut self) -> Option<Utf8Char> {
        let url_str = self.remaining.as_str();
        self.remaining.next().map(|c| (c, &url_str[..c.len_utf8()]))
    }

    pub(crate) fn first_valid(&mut self) -> Option<char> {
        self.remaining
            .by_ref()
            .find(|&c| !matches!(c, '\t' | '\r' | '\n'))
    }
}

impl<'a> Iterator for UrlChars<'a> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        self.remaining.next()
    }
}

pub(crate) fn should_percent_encoding(byte: u8, bytes: &[u8]) -> bool {
    !bytes.is_ascii() || byte < 0x20 || byte == 0x7f || byte >= 0x80 || bytes.contains(&byte)
}

pub(crate) fn percent_hex(byte: u8) -> &'static str {
    static HEX_ASCII: &[u8; 512] = b"\
      000102030405060708090A0B0C0D0E0F\
      101112131415161718191A1B1C1D1E1F\
      202122232425262728292A2B2C2D2E2F\
      303132333435363738393A3B3C3D3E3F\
      404142434445464748494A4B4C4D4E4F\
      505152535455565758595A5B5C5D5E5F\
      606162636465666768696A6B6C6D6E6F\
      707172737475767778797A7B7C7D7E7F\
      808182838485868788898A8B8C8D8E8F\
      909192939495969798999A9B9C9D9E9F\
      A0A1A2A3A4A5A6A7A8A9AAABACADAEAF\
      B0B1B2B3B4B5B6B7B8B9BABBBCBDBEBF\
      C0C1C2C3C4C5C6C7C8C9CACBCCCDCECF\
      D0D1D2D3D4D5D6D7D8D9DADBDCDDDEDF\
      E0E1E2E3E4E5E6E7E8E9EAEBECEDEEEF\
      F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF\
      ";
    let index = usize::from(byte) * 2;
    unsafe { str::from_utf8_unchecked(&HEX_ASCII[index..index + 2]) }
}

#[cfg(test)]
mod ut_uri_percent_encoder {
    use crate::request::uri::percent_encoding::PercentEncoder;
    use crate::request::uri::{InvalidUri, Uri};

    macro_rules! err_percent_encode {
        ($url:expr, $err:expr) => {{
            let encoded = PercentEncoder::parse($url).err();
            assert_eq!(encoded, $err);
        }};
    }

    macro_rules! success_percent_encode {
        ($url:expr, $encoded:expr) => {{
            let encoded = PercentEncoder::parse($url).unwrap();
            assert_eq!(encoded, $encoded);
        }};
    }

    /// UT test cases for `PercentEncoder::parse`.
    ///
    /// # Brief
    /// 1. Creates PercentEncoder by calling PercentEncoder::new().
    /// 2. parse an url that contains chinese.
    /// 3. Checks if the test result is correct by assert_eq!().
    #[test]
    fn url_percent_encode() {
        success_percent_encode!(
            "https://测试名:测试密码@www.example.com/data/new-测试文件.txt?from=project-名称#fragment-百分比-encode",
            "https://%E6%B5%8B%E8%AF%95%E5%90%8D:%E6%B5%8B%E8%AF%95%E5%AF%86%E7%A0%81@www.example.com/data/new-%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt?from=project-%E5%90%8D%E7%A7%B0#fragment-%E7%99%BE%E5%88%86%E6%AF%94-encode"
        );

        success_percent_encode!(
            "https://@www.example.com/data/new-测试文件.txt?from=project-名称#fragment-百分比-encode",
            "https://www.example.com/data/new-%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt?from=project-%E5%90%8D%E7%A7%B0#fragment-%E7%99%BE%E5%88%86%E6%AF%94-encode"
        );

        success_percent_encode!(
            "https://www.example.com/data/new-测试文件.txt#fragment-百分比-encode",
            "https://www.example.com/data/new-%E6%B5%8B%E8%AF%95%E6%96%87%E4%BB%B6.txt#fragment-%E7%99%BE%E5%88%86%E6%AF%94-encode"
        )
    }

    /// UT test cases for `PercentEncoder::parse`.
    ///
    /// # Brief
    /// 1. Creates PercentEncoder by calling PercentEncoder::new().
    /// 2. parse an url that is wrong.
    /// 3. Checks if the test result is correct by assert_eq!().
    #[test]
    fn url_percent_encode_failure() {
        err_percent_encode!(
            "htt ps://测试名:测试密码@www.example.com/data/new-测试文件.txt?from=project-名称#fragment-百分比-encode",
            Some(InvalidUri::InvalidScheme.into())
        );
        err_percent_encode!("htt ps://", Some(InvalidUri::InvalidScheme.into()));
        err_percent_encode!("https", Some(InvalidUri::InvalidScheme.into()));
        err_percent_encode!(
            "https:///www.example.com",
            Some(InvalidUri::InvalidScheme.into())
        );
        err_percent_encode!("https://@/data", Some(InvalidUri::UriMissHost.into()))
    }
}
