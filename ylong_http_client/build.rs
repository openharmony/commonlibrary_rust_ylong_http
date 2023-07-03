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

//! This crate depends on Openssl3.0.
//! Sets environment variables when use the feature `c_openssl_3_0`.
//! Needs export ``OPENSSL_LIB_DIR`` and ``OPENSSL_INCLUDE_DIR``.
//! ``OPENSSL_LIB_DIR`` is the path for ``libssl.so`` and ``libcrypto.so``.
//! ``OPENSSL_INCLUDE_DIR`` is the path for the Openssl header file.

use std::env;

fn main() {
    let lib_dir = env::var("OPENSSL_LIB_DIR");
    let include_dir = env::var("OPENSSL_INCLUDE_DIR");

    if let Ok(lib_dir) = lib_dir {
        println!("cargo:rustc-link-lib=ssl");
        println!("cargo:rustc-link-lib=crypto");
        println!("cargo:rustc-link-search=native={lib_dir}");
    }

    if let Ok(include_dir) = include_dir {
        println!("cargo:include={include_dir}");
    }
}
