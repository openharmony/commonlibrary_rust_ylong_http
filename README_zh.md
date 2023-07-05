# ylong_http

## 简介

ylong_http 协议栈构建了完整的 HTTP 能力，支持用户使用 HTTP 能力完成通信场景的需求。

ylong_http 协议栈主体使用 Rust 语言编写，为 OpenHarmony 的 Rust 能力构筑提供支持。

### ylong_http 在 OpenHarmony 中的位置

ylong_http 向 OpenHarmony 系统服务层中的网络协议栈模块提供 HTTP 协议支持，经由网络协议栈模块帮助上层应用建立 HTTP 通信能力。

![structure](./figures/structure.png)

以下是对于上图关键字段的描述信息：

- `APP`：需要使用上传下载能力的直接面向用户的上层应用。
- `request`：OpenHarmony 系统服务层提供上传下载能力的组件。
- `netstack`：OpenHarmony 系统服务层提供网络协议栈功能的系统组件。
- `ylong_http`：OpenHarmony 系统服务层提供 HTTP 协议栈功能的系统组件，使用 Rust 编写。
  - `ylong_http_client`：`ylong_http` 下的模块之一，提供 HTTP 客户端能力。
  - `ylong_http`:`ylong_http` 下的模块之一，提供 HTTP 的基础组件。
- `ylong_runtime`：`ylong` 在系统服务层提供的 Rust 异步运行时库。
- `tokio`：业界常用的第三方 Rust 异步运行时库。
- `OpenSSL`：业界常用的第三方 TLS 实现库, C 语言实现。

### ylong_http 的内部架构:

![inner_structure](./figures/inner_structure.png)

`ylong_http` 当前内部整体分为两个主要模块：`ylong_http_client` 客户端模块和 `ylong_http` 协议组件模块。

`ylong_http_client` 模块负责提供 HTTP 客户端功能，能够支撑用户发送 HTTP 请求，并返回 HTTP 响应。

`ylong_http_client` 内部又分为三个主要部分:
 - `sync_impl`：同步的 HTTP 客户端实现，该客户端实现不依赖于任何运行时，可以直接在线程模型上运行，但是整体使用同步阻塞策略。
 - `async_impl`：异步的 HTTP 客户端实现，该客户端实现需要使用 Rust 的异步运行时组件。异步 HTTP 客户端利用 Rust 的异步能力能够提供优异的性能表现。
 - `Util`：同步和异步的 HTTP 客户端有一部分实现是共通的，所以这部分通用实现放在该部分中，例如自动重定向实现、HTTP 代理实现等。

`sync_impl` 和 `async_impl` 虽然整体实现是两个形式，但是接口的原型基本一致（主要是 Rust 异步语法与同步语法的差异）。由于接口基本一致所以用户可以在较小的代码改动量之下完成同步和异步逻辑的切换。

`sync_impl` 和 `async_impl` 的整体架构相同，分为如下模块：
 - `Client`：对外提供 HTTP 客户端的基本接口，例如配置客户端的相关选项，发送 HTTP 请求等。
 - `ConnectionPool`：主要负责大量连接管理，管理所有 `Dispatcher` 的生命周期，包括启动、运行、停止。HTTP 协议是基于各种 IO 连接的通信协议，所以可能涉及连接复用、连接管理等功能。
 - `Dispatcher`：主要负责单一连接管理，管理单个连接的启动、运行、停止、传输。每个连接都被一个 `Dispatcher` 管辖，由 `Dispatcher` 决定当前待发送的请求是不是走它管理的连接。
 - `Connections`：真正的连接对象，可以是 TCP 连接、TLS 连接或者是更加泛化的连接对象，在该连接上进行消息传输和接收，是 `Client` 和 HTTP 协议的底座。
 - `Connector`：负责创建连接对象。这一部分提供了 trait，因此可以由用户定义创建连接时的行为。

`Util` 中包含了同步和异步的 HTTP 客户端共通的能力，例如：
- `Redirect`：HTTP 自动重定向能力，当 HTTP 响应返回重定向相关的状态码时，HTTP 客户端会进行自动重定向，自动发送新的请求到下一跳。
- `Proxy`：HTTP 代理能力，发送 HTTP 请求时，向代理发送而非直接发送给原始服务器，然后由代理服务器返回响应。
- `Pool`：通用的连接池实现，支持多个同步或异步连接的管理，便于上层同步或异步客户端复用已有连接，以减少连接重复创建次数以提高性能。
- `OpenSSL_adapter`：HTTPS 需要在 HTTP 的基础上使用 TLS 能力，在 OpenHarmony 上使用 OpenSSL 的 TLS 能力，所以需要对 OpenSSL 的接口进行 Rust 封装才可以被 Rust 使用。

`ylong_http` 模块负责提供 HTTP 的基础能力，例如 HTTP2 的 HPACK、HTTP3 的 QPACK 等。

`ylong_http` 主要包含以下关键模块：
- `Request`：HTTP 请求基础能力，根据 `RFC9110` 规定实现了 HTTP 请求的所有内容和行为。HTTP 请求主要用于向指定服务器发送请求，以获取服务器资源。
- `Response`：HTTP 响应基础能力，根据 `RFC9110` 规定实现了 HTTP 响应的所有内容和行为。HTTP 响应主要是服务器针对客户端发送请求的回应，以返回服务器产生的结果。
- `Body`：
   HTTP 消息体能力，根据 `RFC9110` 规定实现了 HTTP 消息体的所有内容和行为。HTTP 消息体内保存了主要的数据内容，以便客户端和服务器通信。
   HTTP 消息体在协议中有多种形式，在 `ylong_http` 库中也有实现，例如 `EmptyBody` 对应于空消息体，`TextBody` 对应于明文消息体，`ChunkBody` 对应于分块消息体，`Mime` 对应于 Multipart 消息体。
- `H1`：HTTP1 的所有基础能力，例如 HTTP1 格式的请求和响应的序列化器和反序列化器等。
- `H2`：HTTP2 的所有基础能力，例如 HTTP2 格式的请求和响应的序列化器和反序列化器、HTTP2 帧序列化器、HPACK等。
- `H3`：HTTP3 的所有基础能力，例如 HTTP3 格式的请求和响应的序列化器和反序列化器、QPACK 等。

## 编译构建

若使用 GN 编译工具链, 在 ```BUILD.gn``` 的 ```deps``` 段下添加依赖。添加后使用 GN 进行编译和构建：

```gn
deps += ["//example_path/ylong_http_client:ylong_http_client"]
```

若使用 Cargo 编译工具链, 在 ```Cargo.toml``` 下添加依赖。添加后使用 ```cargo``` 进行编译和构建：

```toml
[dependencies]
ylong_http_client = { path = "/example_path/ylong_http_client" } # 请使用路径依赖
```

## 目录

```
ylong_http
├── docs                        # ylong_http 用户指南
├── figures                     # ylong_http 图片资源
├── patches                     # ylong_http 门禁使用的补丁资源
├── ylong_http
│   ├── examples                # ylong_http 基础组件库代码示例
│   ├── src                     # ylong_http 基础组件库源码
│   │   ├── body                # Body trait 定义和扩展 Body 类型
│   │   ├── h1                  # HTTP/1.1 相关组件实现
│   │   ├── h2                  # HTTP/2 相关组件实现
│   │   ├── h3                  # HTTP/3 相关组件实现
│   │   ├── huffman             # Huffman 编解码实现
│   │   ├── request             # Request 定义和实现
│   │   └── response            # Response 定义和实现
│   └── tests                   # ylong_http 基础组件库测试目录
│
└── ylong_http_client
    ├── examples                # ylong_http_client 库代码示例
    ├── src                     # ylong_http_client 库源码
    │   ├── async_impl          # ylong_http_client 异步客户端实现
    │   │   ├── conn            # 异步连接层
    │   │   ├── downloader      # 异步下载器实现
    │   │   ├── ssl_stream      # 异步 tls 适配层
    │   │   └── uploader        # 异步上传器实现   
    │   ├── sync_impl           # ylong_http_client 同步客户端实现
    │   │   └── conn            # 同步连接层
    │   └── util                # ylong_http_client 组件实现
    │       ├── c_openssl       # OpenSSL 封装层
    │       │   ├── ffi         # ffi 封装层
    │       │   └── ssl         # ssl 适配层
    │       └── config          # 配置选项实现
    │           └── tls         # TLS 选项实现
    │               └── alpn    # ALPN 实现
    └── tests                   # ylong_http_client 库测试目录
```

## 用户指南

详细内容请见[用户指南](./docs/user_guide.md)