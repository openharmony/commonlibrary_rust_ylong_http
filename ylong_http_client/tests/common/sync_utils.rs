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
macro_rules! sync_client_test_case {
    (
        HTTPS;
        Tls: $tls_config: expr,
        RuntimeThreads: $thread_num: expr,
        $(ClientNum: $client_num: expr,)?
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        define_service_handle!(HTTPS;);
        set_server_fn!(
            SYNC;
            ylong_server_fn,
            $(Request: {
                Method: $method,
                $(
                    Header: $req_n, $req_v,
                )*
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        let runtime = init_test_work_runtime($thread_num);
        // The number of servers may be variable based on the number of servers set by the user.
        // However, cliipy checks that the variable does not need to be variable.
        #[allow(unused_mut, unused_assignments)]
        let mut server_num = 1;
        $(server_num = $client_num;)?

        let mut handles_vec = vec![];
        start_server!(
            HTTPS;
            ServerNum: server_num,
            Runtime: runtime,
            Handles: handles_vec,
            ServeFnName: ylong_server_fn,
        );

        let mut shut_downs = vec![];
        sync_client_assert!(
            HTTPS;
            Tls: $tls_config,
            Runtime: runtime,
            ServerNum: server_num,
            Handles: handles_vec,
            ShutDownHandles: shut_downs,
            $(Request: {
                Method: $method,
                Host: $host,
                $(
                    Header: $req_n, $req_v,
                )*
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        for shutdown_handle in shut_downs {
            runtime.block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
        }
    }};
    (
        HTTP;
        RuntimeThreads: $thread_num: expr,
        $(ClientNum: $client_num: expr,)?
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
                Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
                Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        define_service_handle!(HTTP;);
        set_server_fn!(
            SYNC;
            ylong_server_fn,
            $(Request: {
                Method: $method,
                $(
                    Header: $req_n, $req_v,
                )*
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        let runtime = init_test_work_runtime($thread_num);
        // The number of servers may be variable based on the number of servers set by the user.
        // However, cliipy checks that the variable does not need to be variable.
        #[allow(unused_mut, unused_assignments)]
        let mut server_num = 1;
        $(server_num = $client_num;)?
        let mut handles_vec = vec![];

        start_server!(
            HTTP;
            ServerNum: server_num,
            Runtime: runtime,
            Handles: handles_vec,
            ServeFnName: ylong_server_fn,
        );

        let mut shut_downs = vec![];
        sync_client_assert!(
            HTTP;
            Runtime: runtime,
            ServerNum: server_num,
            Handles: handles_vec,
            ShutDownHandles: shut_downs,
            $(Request: {
                Method: $method,
                Host: $host,
                $(
                    Header: $req_n, $req_v,
                )*
                Body: $req_body,
            },
            Response: {
                Status: $status,
                Version: $version,
                $(
                    Header: $resp_n, $resp_v,
                )*
                Body: $resp_body,
            },)*
        );

        for shutdown_handle in shut_downs {
            runtime.block_on(shutdown_handle).expect("Runtime wait for server shutdown failed");
        }
    }};

}

#[macro_export]
macro_rules! sync_client_assert {
    (
        HTTPS;
        Tls: $tls_config: expr,
        Runtime: $runtime: expr,
        ServerNum: $server_num: expr,
        Handles: $handle_vec: expr,
        ShutDownHandles: $shut_downs: expr,
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        let client = ylong_http_client::sync_impl::Client::builder()
            .tls_ca_file($tls_config)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap();
        let client = std::sync::Arc::new(client);
        for _i in 0..$server_num {
            let handle = $handle_vec.pop().expect("No more handles !");
            let client = std::sync::Arc::clone(&client);
            sync_client_assertions!(
                ServerHandle: handle,
                ClientRef: client,
                $(Request: {
                    Method: $method,
                    Host: $host,
                    $(
                        Header: $req_n, $req_v,
                    )*
                    Body: $req_body,
                },
                Response: {
                    Status: $status,
                    Version: $version,
                    $(
                        Header: $resp_n, $resp_v,
                    )*
                    Body: $resp_body,
                },)*
            );
            let shutdown_handle = $runtime.spawn(async move {

            });
            $shut_downs.push(shutdown_handle);
        }
    }};
    (
        HTTP;
        Runtime: $runtime: expr,
        ServerNum: $server_num: expr,
        Handles: $handle_vec: expr,
        ShutDownHandles: $shut_downs: expr,
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {{
        let client = ylong_http_client::sync_impl::Client::new();
        let client = Arc::new(client);
        for _i in 0..$server_num {
            let mut handle = $handle_vec.pop().expect("No more handles !");
            let client = std::sync::Arc::clone(&client);
            sync_client_assertions!(
                ServerHandle: handle,
                ClientRef: client,
                $(Request: {
                    Method: $method,
                    Host: $host,
                    $(
                        Header: $req_n, $req_v,
                    )*
                    Body: $req_body,
                },
                Response: {
                    Status: $status,
                    Version: $version,
                    $(
                        Header: $resp_n, $resp_v,
                    )*
                    Body: $resp_body,
                },)*
            );
            let shutdown_handle = $runtime.spawn(async move {
                ensure_server_shutdown!(ServerHandle: handle);
            });
            $shut_downs.push(shutdown_handle);
        }

    }}
}

#[macro_export]
macro_rules! sync_client_assertions {
    (
        ServerHandle: $handle:expr,
        ClientRef: $client:expr,
        $(Request: {
            Method: $method: expr,
            Host: $host: expr,
            $(
            Header: $req_n: expr, $req_v: expr,
            )*
            Body: $req_body: expr,
        },
        Response: {
            Status: $status: expr,
            Version: $version: expr,
            $(
            Header: $resp_n: expr, $resp_v: expr,
            )*
            Body: $resp_body: expr,
        },)*
    ) => {
        $(
            let request = ylong_request!(
                Request: {
                    Method: $method,
                    Host: $host,
                    Port: $handle.port,
                    $(
                        Header: $req_n, $req_v,
                    )*
                    Body: $req_body,
                },
            );
            let mut response = $client
                .request(request)
                .expect("Request send failed");
            assert_eq!(response.status().as_u16(), $status, "Assert response status code failed");
            assert_eq!(response.version().as_str(), $version, "Assert response version failed");
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
                    .body_mut()
                    .data(&mut buf[size..])
                    .expect("Response body read failed");
                if read == 0 {
                    break;
                }
                size += read;
            }
            assert_eq!(&buf[..size], $resp_body.as_bytes(), "Assert response body failed");
        )*
    };
}
