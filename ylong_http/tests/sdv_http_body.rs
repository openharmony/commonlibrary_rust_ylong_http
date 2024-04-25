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

#![cfg(all(feature = "http1_1", feature = "ylong_base"))]

use ylong_http::body::{async_impl, ChunkBody, ChunkBodyDecoder, EmptyBody};
use ylong_http::headers::Headers;

fn content_message() -> Vec<u8> {
    let mut vec = Vec::new();
    for i in 0..=9 {
        vec.extend_from_slice(&[i % 10; 100]);
    }
    vec.extend_from_slice(&[1; 100]);
    vec
}

fn init_chunk() -> Vec<u8> {
    let mut res = b"400\r\n".to_vec();
    for i in 0..=9 {
        res.extend_from_slice(&[i % 10; 100]);
    }
    res.extend_from_slice(&[1; 24]);
    res.extend_from_slice(b"\r\n4c\r\n");
    res.extend_from_slice(&[1; 76]);
    res.extend_from_slice(b"\r\n0\r\n");
    res
}

fn append_trailer(mut res: Vec<u8>) -> Vec<u8> {
    res.extend_from_slice(b"accept:text/html\r\n");
    res
}

fn chunk_finish(mut res: Vec<u8>) -> Vec<u8> {
    res.extend_from_slice(b"\r\n");
    res
}

/// SDV test cases for `ChunkBody::data`.
///
/// # Brief
/// 1. Creates a `ChunkBody` by calling `ChunkBody::from_reader`.
/// 2. Encodes chunk body by calling `ChunkBody::data`
/// 3. Checks if the test result is correct.
#[test]
fn sdv_chunk_body_encode() {
    use ylong_http::body::sync_impl::Body as SyncBody;

    let content = content_message();
    let mut body = ChunkBody::from_reader(content.as_slice());
    let mut buf = [0_u8; 20];
    let mut output = vec![];

    let mut size = buf.len();
    while size == buf.len() {
        size = body.data(buf.as_mut_slice()).unwrap();
        output.extend_from_slice(&buf[..size]);
    }
    assert_eq!(output, chunk_finish(init_chunk()));
}

/// SDV test cases for `ChunkBody::data` in async condition.
///
/// # Brief
/// 1. Creates a `ChunkBody` by calling `ChunkBody::from_async_reader`.
/// 2. Encodes chunk body by calling `async_impl::Body::data`
/// 3. Checks if the test result is correct.
#[cfg(feature = "ylong_base")]
#[test]
fn sdv_async_chunk_body_from_async_reader() {
    let handle = ylong_runtime::spawn(async move {
        let content = content_message();
        let mut task = ChunkBody::from_async_reader(content.as_slice());
        let mut buf = [0_u8; 1024];
        let mut output = vec![];

        let mut size = buf.len();
        while size == buf.len() {
            size = async_impl::Body::data(&mut task, buf.as_mut_slice())
                .await
                .unwrap();
            output.extend_from_slice(&buf[..size]);
        }
        assert_eq!(output, chunk_finish(init_chunk()));
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `ChunkBody::data` in async condition.
///
/// # Brief
/// 1. Creates a `ChunkBody` by calling `ChunkBody::from_bytes`.
/// 2. Encodes chunk body by calling `async_impl::Body::data`
/// 3. Checks if the test result is correct.
#[cfg(feature = "ylong_base")]
#[test]
fn sdv_async_chunk_body_from_bytes() {
    let handle = ylong_runtime::spawn(async move {
        let content = content_message();
        let mut task = ChunkBody::from_bytes(content.as_slice());
        let mut buf = [0_u8; 20];
        let mut output = vec![];

        let mut size = buf.len();
        while size == buf.len() {
            size = async_impl::Body::data(&mut task, buf.as_mut_slice())
                .await
                .unwrap();
            output.extend_from_slice(&buf[..size]);
        }
        assert_eq!(output, chunk_finish(init_chunk()));
    });
    ylong_runtime::block_on(handle).unwrap();
}

/// SDV test cases for `ChunkBody::set_trailer`.
///
/// # Brief
/// 1. Creates a `ChunkBody` by calling `ChunkBody::set_trailer`.
/// 2. Encodes chunk body by calling `ChunkBody::data`
/// 3. Checks if the test result is correct.
#[test]
fn sdv_chunk_body_encode_trailer_0() {
    use ylong_http::body::sync_impl::Body as SyncBody;

    let mut headers = Headers::new();
    let _ = headers.insert("accept", "text/html");
    let content = content_message();
    let mut task = ChunkBody::from_bytes(content.as_slice()).set_trailer(headers);
    let mut buf = [0_u8; 20];
    let mut output = vec![];
    let mut size = buf.len();
    while size == buf.len() {
        size = task.data(buf.as_mut_slice()).unwrap();
        output.extend_from_slice(&buf[..size]);
    }
    assert_eq!(output, chunk_finish(append_trailer(init_chunk())));
}

/// SDV test cases for `ChunkBodyDecoder::decode`.
///
/// # Brief
/// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
/// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
/// 3. Checks if the test result is correct.
#[test]
fn sdv_chunk_body_decode_0() {
    let mut decoder = ChunkBodyDecoder::new();
    let chunk_body_bytes_1 = "\
            2\r\n\
            hi\r\n\
            2; type = text ;end = hi\r\n\
            hi\r\n\
            0; ext = last\r\n\
            \r\n\
            "
    .as_bytes();

    // 5
    let (chunks_1, _) = decoder.decode(chunk_body_bytes_1).unwrap();
    let chunk_body_bytes_2 = "\
            2\r\n\
            hi\r\n\
            2; type = text\r\n\
            hi\r\n\
            0\r\n\
            \r\n\
            "
    .as_bytes();

    // 5
    let (chunks_2, _) = decoder.decode(chunk_body_bytes_2).unwrap();
    assert_eq!(
        format!("{:?}", chunks_1),
        "Chunks { chunks: [Chunk { \
    id: 0, state: Finish, size: 2, extension: ChunkExt { map: {} }, \
    data: [104, 105], trailer: None }, \
    Chunk { id: 1, state: Finish, size: 2, extension: ChunkExt { map: {} }, \
    data: [104, 105], trailer: None }, \
    Chunk { id: 2, state: Finish, size: 0, \
    extension: ChunkExt { map: {} }, data: [], trailer: None }] }"
    );
    assert_eq!(chunks_1, chunks_2);

    assert_eq!(chunks_1.iter().len(), 3);
    let chunk_1 = chunks_1.iter().next().unwrap();
    assert_eq!(format!("{:?}", chunk_1), "Chunk { id: 0, state: Finish, size: 2, extension: ChunkExt { map: {} }, data: [104, 105], trailer: None }");
    let chunk_2 = chunks_1.iter().next().unwrap();
    assert_eq!(*chunk_1, *chunk_2);
}

/// SDV test cases for `async_impl::Body::data` of `EmptyBody`.
///
/// # Brief
/// 1. Creates an `EmptyBody`.
/// 2. Calls its `async_impl::Body::data` method and then checks the results.
#[cfg(feature = "ylong_base")]
#[test]
fn sdv_empty_body_async_impl_data() {
    use ylong_http::body::async_impl::Body as AsyncBody;

    let handle = ylong_runtime::spawn(async move {
        let mut body = EmptyBody;
        let mut buf = [0u8; 1];
        assert_eq!(body.data(&mut buf).await, Ok(0));
    });
    ylong_runtime::block_on(handle).unwrap();
}
