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

//! Tests nested `MimeMulti`, for its building and asynchronous encoding.

use ylong_http::body::{async_impl, MimeMulti, MimeMultiEncoder, MimePart};

#[tokio::main]
async fn main() {
    let body_text1 = "\
    This could have been part of the previous part, but \
    illustrates explicit versus implicit typing of body parts.\r\n"
        .as_bytes();
    let body_text2 = "\
    ... base64-encoded 8000 Hz single-channel \
    mu-law-format audio data goes here ...\r\n"
        .as_bytes();
    let body_text3 = "... base64-encoded image data goes here ...\r\n".as_bytes();

    let body_text4 = "\
This is <bold><italic>enriched.</italic></bold>
<smaller>as defined in RFC 1896</smaller>

Isn't it
<bigger><bigger>cool?</bigger></bigger>
"
    .as_bytes();

    let body_text5 = "\
From: (mailbox in US-ASCII)
To: (address in US-ASCII)
Subject: (subject in US-ASCII)
Content-Type: Text/plain; charset=ISO-8859-1
Content-Transfer-Encoding: Quoted-printable

... Additional text in ISO-8859-1 goes here ...
"
    .as_bytes();

    let multi = MimeMulti::builder()
        .set_content_type(b"multipart/mixed", b"unique-boundary-1".to_vec())
        .add_part(
            MimePart::builder()
                .body_from_bytes("... Some text appears here ...\r\n".as_bytes())
                .build()
                .unwrap(),
        )
        .add_part(
            MimePart::builder()
                .header("Content-type", "text/plain; charset=US-ASCII")
                .body_from_bytes(body_text1)
                .build()
                .unwrap(),
        )
        .add_multi(
            MimeMulti::builder()
                .set_content_type(b"multipart/parallel", b"unique-boundary-2".to_vec())
                .add_part(
                    MimePart::builder()
                        .header("Content-type", "audio/basic")
                        .header("Content-Transfer-Encoding", "base64")
                        .body_from_bytes(body_text2)
                        .build()
                        .unwrap(),
                )
                .add_part(
                    MimePart::builder()
                        .header("Content-type", "image/jpeg")
                        .header("Content-Transfer-Encoding", "base64")
                        .body_from_bytes(body_text3)
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .add_part(
            MimePart::builder()
                .header("Content-type", "text/enriched")
                .body_from_async_reader(body_text4)
                .build()
                .unwrap(),
        )
        .add_part(
            MimePart::builder()
                .header("Content-type", "message/rfc822")
                .body_from_async_reader(body_text5)
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let mut multi_encoder = MimeMultiEncoder::from_multi(multi);
    let mut buf = vec![0u8; 50];
    let mut v_size = vec![];
    let mut v_str = vec![];

    loop {
        let len = async_impl::Body::data(&mut multi_encoder, &mut buf)
            .await
            .unwrap();
        if len == 0 {
            break;
        }
        v_size.push(len);
        v_str.extend_from_slice(&buf[..len]);
    }
    assert_eq!(
        v_size,
        vec![
            50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50,
            35
        ]
    );
    // Headers is a HashMap, so that sequence of iter is different.
    println!("{}", core::str::from_utf8(&v_str).unwrap());
}
