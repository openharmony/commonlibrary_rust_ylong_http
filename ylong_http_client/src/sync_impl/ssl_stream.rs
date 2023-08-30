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

use std::io::{Read, Write};

#[cfg(feature = "__c_openssl")]
use crate::util::c_openssl::ssl::SslStream;

/// A stream which may be wrapped with TLS.
pub enum MixStream<T> {
    /// A raw HTTP stream.
    Http(T),
    /// An SSL-wrapped HTTP stream.
    Https(SslStream<T>),
}

impl<T> Read for MixStream<T>
where
    T: Read + Write,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match &mut *self {
            MixStream::Http(s) => s.read(buf),
            MixStream::Https(s) => s.read(buf),
        }
    }
}
impl<T> Write for MixStream<T>
where
    T: Read + Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut *self {
            MixStream::Http(s) => s.write(buf),
            MixStream::Https(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut *self {
            MixStream::Http(s) => s.flush(),
            MixStream::Https(s) => s.flush(),
        }
    }
}
