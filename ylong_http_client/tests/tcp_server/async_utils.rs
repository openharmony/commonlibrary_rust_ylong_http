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

#[macro_export]
macro_rules! async_client_test_on_tcp {
    (
        HTTP;
        $(ClientNum: $client_num: expr,)?
        $(Request: {
            Method: $method: expr,
            Version: $req_version: expr,
            Path: $path: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $resp_version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{

        // The number of servers may be variable based on the number of servers set by the user.
        // However, clippy checks that the variable does not need to be variable.
        #[allow(unused_mut, unused_assignments)]
        let mut server_num = 1;
        $(server_num = $client_num;)?
        let mut handles_vec = vec![];

        start_tcp_server!(
            ASYNC;
            Proxy: false,
            ServerNum: server_num,
            Handles: handles_vec,
            $(Request: {
                Method: $method,
                Version: $req_version,
                Path: $path,
                $(
                    Header: $req_n, $req_v,
                )*
                Header: "Accept", "*/*",
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $resp_version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        let mut shut_downs = vec![];
        async_client_assert_on_tcp!(
            HTTP;
            ServerNum: server_num,
            Handles: handles_vec,
            ShutDownHandles: shut_downs,
            $(Request: {
                Method: $method,
                Version: $req_version,
                Path: $path,
                $(
                    Header: $req_n, $req_v,
                )*
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $resp_version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        for shutdown_handle in shut_downs {
            ylong_runtime::block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
        }
    }};

}

#[macro_export]
macro_rules! async_client_assert_on_tcp {
    (
        HTTP;
        ServerNum: $server_num: expr,
        Handles: $handle_vec: expr,
        ShutDownHandles: $shut_downs: expr,
        $(Request: {
            Method: $method: expr,
            Version: $req_version: expr,
            Path: $path: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $resp_version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        let client = ylong_http_client::async_impl::Client::new();
        let client = std::sync::Arc::new(client);
        for _i in 0..$server_num {
            let handle = $handle_vec.pop().expect("No more handles !");
            let client = std::sync::Arc::clone(&client);
            let shutdown_handle = ylong_runtime::spawn(async move {
                async_client_assertions_on_tcp!(
                    ServerHandle: handle,
                    ClientRef: client,
                    $(Request: {
                        Method: $method,
                        Version: $req_version,
                        Path: $path,
                        $(
                            Header: $req_n, $req_v,
                        )*
                        Body: $req_body,
                    },
                    Response: {
                        Status: $status,
                        Version: $resp_version,
                        $(
                            Header: $resp_n, $resp_v,
                        )*
                        Body: $resp_body,
                    },)*
                );
                handle.server_shutdown.recv().expect("server send order failed !");
            });
            $shut_downs.push(shutdown_handle);
        }
    }}
}

#[macro_export]
macro_rules! async_client_assertions_on_tcp {
    (
        ServerHandle: $handle:expr,
        ClientRef: $client:expr,
        $(Request: {
            Method: $method: expr,
            Version: $req_version: expr,
            Path: $path: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $resp_version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {
        $(
            {
                let request = build_client_request!(
                    Request: {
                        Method: $method,
                        Version: $req_version,
                        Path: $path,
                        Addr: $handle.addr.as_str(),
                        $(
                            Header: $req_n, $req_v,
                        )*
                        Body: $req_body,
                    },
                );

                let mut response = $client
                    .request(request)
                    .await
                    .expect("Request send failed");

                assert_eq!(response.status().as_u16(), $status, "Assert response status code failed") ;
                assert_eq!(response.version().as_str(), $resp_version, "Assert response version failed");
                $(assert_eq!(
                    response
                        .headers()
                        .get($resp_n)
                        .expect(format!("Get response header \"{}\" failed", $resp_n).as_str())
                        .to_string()
                        .expect(format!("Convert response header \"{}\"into string failed", $resp_n).as_str()),
                    $resp_v,
                    "Assert response header \"{}\" failed", $resp_n,
                );)*
                let mut buf = [0u8; 4096];
                let mut size = 0;
                loop {
                    let read = response
                        .data(&mut buf[size..]).await
                        .expect("Response body read failed");
                    if read == 0 {
                        break;
                    }
                    size += read;
                }
                assert_eq!(&buf[..size], $resp_body.as_bytes(), "Assert response body failed");
            }
        )*
    }
}
