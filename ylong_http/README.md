# ylong_http

## 简介

ylong_http 提供了 HTTP 各个版本下的协议所需的各种基础组件和扩展组件，方便用户组织所需的 HTTP 结构。

ylong_http 包含以下核心功能：

### Request 和 Response

ylong_http 使用 `Request` 和 `Response` 结构来表示 HTTP 最基础的请求和响应：

- `Request`：HTTP 请求，包含 `Method`、`Uri`、`Headers`、`Body` 等。

- `Response`：HTTP 响应，包含 `StatusCode`、`Version`、`Headers`、`Body `等。

用户可以使用 `Request` 和 `Response` 提供的相关方法来获取相关信息，或是自定义请求和响应。



### Body

对于`Request`和`Response`的 Body 部分，ylong_http 提供了 `Body` trait，方便用户自定义想要的 Body 结构。

为了区分 Body 结构所处在同步还是异步上下文，ylong_http 声明了 `sync_impl::Body` trait 和 `async_impl::Body` trait，使用 mod 进行隔离：

- `sync_impl::Body` ：该 trait 使用同步方法，可以在同步上下文使用。
- `async_impl::Body`：该 trait 使用异步方法，可以在异步上下文使用。

用户可以根据自身需要，选择合适的 `Body` trait 进行实现，也可以两种都实现。

ylong_http 也提供默认的 Body 结构供用户使用：

- `EmptyBody`：空的 Body 结构。
- `TextBody`：明文的 Body 结构，支持用户传入内存数据或是 IO 数据。
- `ChunkBody`：分段的 Body 结构，支持用户自定义 Chunk 大小，支持传入内存数据或是 IO 数据。
- `MimeBody`：MIME 格式的 Body 结构，支持用户使用 MIME 格式设置 Body，支持传入内存数据或是 IO 数据。

对应的，也提供了几种 Body 类型的读取器：

- `TextBodyDecoder`：用于解析明文 Body。
- `ChunkBodyDecoder`：用于解析分段 Body。
- `MimeBodyDecoder`：用于解析 MIME 格式的 Body。



### 其他组件

ylong_http 提供了以下几个 HTTP 版本的相关组件：

- HTTP/1.1：提供了 `RequestEncoder`、`ResponseDecoder` 等。
- HTTP/2：`DynamicTable`、`StaticTable`、`FrameDecoder`、`FrameEncoder` 等。
- HTTP/3：`FrameDecoder`、`FrameEncoder` 等。



## 编译构建

在 ```Cargo.toml``` 下添加依赖。添加后使用 ```cargo``` 进行编译和构建：

```toml
[dependencies]
ylong_http = { path = "/example_path/ylong_http" } # 请使用路径依赖
```



## 目录

```
ylong_http
├── examples                               # ylong_http 代码示例
├── src 
│   ├── body                               # Body trait 定义和扩展 Body 类型。
│   ├── h1                                 # HTTP/1.1 相关组件实现。
│   ├── h2								# HTTP/2 相关组件实现。
│   ├── h2								# HTTP/3 相关组件实现。
│   ├── huffman                            # Huffman 编解码实现。
│   ├── request                            # Request 定义和实现。
│   └── response                           # Response 定义和实现。
└── tests                                  # 测试目录
```



