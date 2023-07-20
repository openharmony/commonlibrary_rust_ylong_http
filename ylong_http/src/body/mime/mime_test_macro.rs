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

/// Builds a `MimePart`.
macro_rules! part_build {
    (
        MimePart: {
            $(Headers: $headers: expr,)?
            $(Header: $name: expr, $value: expr,)*
            $(BodyOwned: $body1: expr,)?
            $(BodySlice: $body2: expr,)?
            $(BodyReader: $body3: expr,)?
            $(BodyAsyncReader: $body4: expr,)?
        },
    ) => {
        MimePart::builder()
            $(.set_headers($headers))?
            $(.header($name, $value))*
            $(.body_from_owned($body1))?
            $(.body_from_bytes($body2))?
            $(.body_from_reader($body3))?
            $(.body_from_async_reader($body4))?
            .build()
            .expect("MimePart build failed")
    }
}

/// Builds a `MimePart`, encodes it, Compares with result.
macro_rules! part_encode_compare {
    (
        MimePart: {
            $(Headers: $headers: expr,)?
            $(Header: $name: expr, $value: expr,)*
            $(BodyOwned: $body1: expr,)?
            $(BodySlice: $body2: expr,)?
            $(BodyReader: $body3: expr,)?
            $(BodyAsyncReader: $body4: expr,)?
        },
        $(BufSize: $size: expr,)?
        $(ResultSize: $v_size: expr,)?
        $(ResultData: $v_data: expr,)?
        Sync,
    ) => {
        let part = part_build!(
            MimePart: {
                $(Headers: $headers: expr,)?
                $(Header: $name, $value,)*
                $(BodyOwned: $body1,)?
                $(BodySlice: $body2,)?
                $(BodyReader: $body3,)?
                $(BodyAsyncReader: $body4,)?
            },
        );

        // default 1
        #[allow(unused_assignments, unused_mut)]
        let mut len = 1;

        $(len = $size;)?
        let mut buf = vec![0u8; len];
        let mut v_data = vec![];
        let mut v_size = vec![];
        let mut part_encoder = MimePartEncoder::from_part(part);

        loop {
            let size = sync_impl::Body::data(&mut part_encoder, &mut buf).expect("MimePart encode failed");
            if size == 0 {
                break;
            }
            v_size.push(size);
            v_data.extend_from_slice(&buf[..size]);
        }
        $(assert_eq!(v_size, $v_size);)?
        $(assert_eq!(v_data, $v_data);)?
    };

    (
        MimePart: {
            $(Headers: $headers: expr,)?
            $(Header: $name: expr, $value: expr,)*
            $(BodyOwned: $body1: expr,)?
            $(BodySlice: $body2: expr,)?
            $(BodyReader: $body3: expr,)?
            $(BodyAsyncReader: $body4: expr,)?
        },
        $(BufSize: $size: expr,)?
        $(ResultSize: $v_size: expr,)?
        $(ResultData: $v_data: expr,)?
        Async,
    ) => {
        let part = part_build!(
            MimePart: {
                $(Headers: $headers: expr,)?
                $(Header: $name, $value,)*
                $(BodyOwned: $body1,)?
                $(BodySlice: $body2,)?
                $(BodyReader: $body3,)?
                $(BodyAsyncReader: $body4,)?
            },
        );
        // default 1
        #[allow(unused_assignments, unused_mut)]
        let mut len = 1;

        $(len = $size;)?
        let mut buf = vec![0u8; len];
        let mut v_data = vec![];
        let mut v_size = vec![];
        let mut part_encoder = MimePartEncoder::from_part(part);

        loop {
            // async
            let size = async_impl::Body::data(&mut part_encoder, &mut buf).await.expect("MimePart encode failed");
            if size == 0 {
                break;
            }
            v_size.push(size);
            v_data.extend_from_slice(&buf[..size]);
        }
        $(assert_eq!(v_size, $v_size);)?
        $(assert_eq!(v_data, $v_data);)?
    };
}
