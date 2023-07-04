# ylong_http 用户指南

ylong_http 提供了 HTTP 各个版本下的协议所需的各种基础组件和扩展组件，方便用户组织所需的 HTTP 结构。

ylong_http 整体分为 2 个库：

- ylong_http_client 库：HTTP 客户端库
- ylong_http 库：HTTP 协议及组件库

其中 ylong_http_client 库提供了 HTTP 客户端的功能，ylong_http 库提供了 HTTP 协议的基础组件。

如果需要查看详细的接口说明请查看对应接口的 docs，可以使用 `cargo doc --open` 生成并查看 docs。

## ylong_http_client

用户可以使用 ylong_http_client 库来创建自定义的客户端。

用户可以使用自定义客户端来向指定服务端发送请求，然后接收响应。

在使用 ylong_http_client 的功能之前，请保证在 `BUILD.gn` 或 `Cargo.toml` 中已成功添加依赖并开启对应 feature。

当前支持的功能：

- 支持异步 HTTP 客户端创建
- 支持 HTTP/1.1
- 支持 HTTPS
- 支持上传下载回调
- 支持简单的 Mime 格式传输

#### 创建一个异步客户端

用户可以使用 `ylong_http_client::async_impl::Client` 来生成一个异步 HTTP 客户端。该功能被 feature `async` 控制。

用户可以使用 `Client::new()` 直接生成默认配置的客户端：

```rust
use ylong_http_client::async_impl::Client;

async fn create_default_client() {
    // 创建一个默认配置选项的客户端。
    let _client_default = Client::new();
}
```

用户也可以使用 `Client::builder()` 自定义客户端：

```rust
use ylong_http_client::async_impl::Client;
use ylong_http_client::Timeout;

async fn create_client_with_builder() {
    let _client_with_builder = Client::builder()    // 创建 builder。
        .connect_timeout(Timeout::from_secs(3))     // 设置一些自定义选项。
        .request_timeout(Timeout::from_secs(3))
        .build();                                   // 构建 Client。
}
```

当前版本提供的 Client 的配置选项：

- `connect_timeout`: 设置连接超时时间
- `request_timeout`: 设置请求超时时间
- `redirect`: 设置重定向逻辑
- `proxy`: 设置代理逻辑
- `tls_built_in_root_certs`: 是否使用预置证书
- `add_root_certificate`: 设置根证书
- `min_tls_version`: 设置 TLS 版本下限
- `max_tls_version`: 设置 TLS 版本上限
- `set_cipher_suite`: 设置 TLSv1.3 的算法套件
- `set_cipher_list`: 设置 TLSv1.3 之前版本的算法套件
- `set_ca_file`: 设置 CA 证书文件路径

#### 创建请求

用户可以使用 `Request` 结构提供的快速接口来生成 HTTP 请求。

```rust
use ylong_http_client::Request;

async fn create_default_request() {
    // 创建一个为 url 为 127.0.0.1:3000，body 为空的 GET 请求。
    let _request_default = Request::get("127.0.0.1:3000").body("".as_bytes()).unwrap();
}
```

用户也可以利用 `Request::builder()` 来自定义请求。

```rust
use ylong_http_client::{Request, Method};

async fn create_request_with_builder() {
    let _request_with_builder = Request::builder()  // 创建 RequestBuilder
        .method(Method::GET)    // 设置 Method
        .url("http://www.example.com")  // 设置 Url
        .header("Content-Type", "application/octet-stream") // 设置 Header
        .body("".as_bytes());   // 设置 body
}
```

当前版本提供的 Request 的配置选项：

- `method`: 设置请求方法
- `url`: 设置 Url
- `header`: 插入请求头字段
- `append_header`: 追加请求头字段
- `body`: 设置一般 body 内容
- `multipart`: 设置 multipart 格式 body 内容

用户可以创建 Multipart 格式的请求：

```rust
use ylong_http_client::async_impl::{Multipart, Part};
use ylong_http_client::Request;

async fn create_multipart() {
    // 创建单个 part
    let part = Part::new()
        .mime("application/octet-stream")
        .name("name")
        .file_name("file_name")
        .length(Some(10))
        .body("HelloWorld");
    
    // 创建 Multipart。
    let multipart = MultiPart::new().part(part);
    
    // 使用 multipart 接口创建 Request。
    let _request = Request::get("127.0.0.1:3000").multipart(multipart);
}
```

用户可以在 body 的基础上，使用 `Uploader` 对上传逻辑进行控制：

```rust
use ylong_http_client::async_impl::Uploader;
use ylong_http_client::Response;

async fn create_uploader() {
    // 创建输出到控制台的 Uploader。
    let _uploader = Uploader::console("HelloWorld".as_bytes());
}
```

```rust
use std::pin::Pin;
use std::task::{Context, Poll};
use ylong_http_client::async_impl::{Uploader, UploadOperator};
use ylong_http_client::HttpClientError;

async fn upload_and_show_progress() {
    // 自定义的 `UploadOperator`.
    struct MyUploadOperator;
    
    impl UploadOperator for MyUploadOperator {
        fn poll_progress(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            uploaded: u64,
            total: Option<u64>
        ) -> Poll<Result<(), HttpClientError>> {
            todo!()
        }
    }
    
    // 根据自定义的 Operator 创建 Uploader。
    let uploader = Uploader::builder().reader("HelloWorld".as_bytes()).operator(MyUploadOperator).build();
}
```

#### 发送请求、等待响应

创建好 `Client` 和 `Request` 后即可利用 `Client::request` 接口发送请求并等待响应。 

```rust
use ylong_http_client::async_impl::Client;
use ylong_http_client::Request;

async fn send_request<T>(client: Client, request: Request<T>) {
    // 发送请求，等待响应。
    let _response = client.request(request).await.unwrap();
}
```

得到的响应会以 `Response` 结构的形式返回。响应的头字段会被完整地解析在 `Response` 结构体中，
但是 body 部分需要用户根据自身需求在后续逻辑中读取。

#### 读取响应 body

得到 `Response` 后，可以对响应的 body 信息读取。

用户可以通过调用 `Response::body_mut()`，获取到 body，再使用 `Body::data()` 自行读取 body 内容。

```rust
use ylong_http_client::async_impl::Body;
use ylong_http_client::{Response, HttpClientError};

async fn receive_response_body(mut response: Response) -> Result<(), HttpClientError> {
    let mut buf = [0u8; 1024];
    loop {
        let size = response.body_mut().data(&mut buf).await?;
        if size == 0 {
            return Ok(())
        }
        let data = &buf[..size];
        // 处理接收到的 data 信息。
    }
}
```

使用 `Downloader` 可以进行一个更加灵活的下载方式。

`Downloader` 提供一个直接将 body 输出到控制台的简单方式：

```rust
use ylong_http_client::async_impl::{Downloader, HttpBody, Response};

async fn download_and_show_progress_on_console(response: Response) {
    // 将 Response body 打印到控制台，以字节方式打印。
    let _ = Downloader::console(response).download().await;
}
```

用户也可以自定义 `Downloader` 中的 `DownloadOperator` 组件来实现灵活的自定义下载操作。

用户需要给自身结构体实现 `DownloadOperator` trait，实现其中的 `poll_download` 和
`poll_progress` 方法，以实现下载和显示回调功能。

```rust
use std::pin::Pin;
use std::task::{Context, Poll};
use ylong_http_client::async_impl::{Downloader, DownloadOperatorResponse};
use ylong_http_client::{HttpClientError, SpeedLimit, Timeout};

async fn download_and_show_progress(response: Response) {
    // 自定义的 `DownloadOperator`.
    struct MyDownloadOperator;

    // 为自定义结构实现 DownloadOperator。
    impl DownloadOperator for MyDownloadOperator {
         fn poll_download(
             self: Pin<&mut Self>,
             cx: &mut Context<'_>,
             data: &[u8],
         ) -> Poll<Result<usize, HttpClientError>> {
             // 自定义 download 函数，每次从 Response 中读取到 data 后都会自动调用该接口。
             todo!()
         }
    
         fn poll_progress(
             self: Pin<&mut Self>,
             cx: &mut Context<'_>,
             downloaded: u64,
             total: Option<u64>
         ) -> Poll<Result<(), HttpClientError>> {
             // 自定义 progress 函数，每次从 Response 中读取到 data 并处理后会调用该接口进行显示回调。
             todo!()
         }
     }

     // 创建 Downloader 对指定 Response 进行下载。
     let mut downloader = Downloader::builder()
         .body(response)    // 设置 response
         .operator(MyDownloadOperator)  // 设置 operator
         .build();
     let _ = downloader.download().await;
}
```