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

以下是对于上图关键字段的描述信息：

- `ylong_http_client` 库：`ylong_http` 下的模块之一，提供 HTTP 客户端能力。
  - `sync_impl`：同步 HTTP 客户端实现，不依赖于任何运行时。
    - `Client`：同步 HTTP 客户端，用于发送 HTTP 请求。
    - `ConnectionPool`：管理所有的 `Dispatcher`。
    - `Dispatcher`：管理 `Connections` 的使用权。
    - `Connector`：用于创建同步连接。
    - `Connections`：同步的 TCP 或 TLS 连接等。
  - `async_impl`：异步 HTTP 客户端实现，不依赖于任何运行时。
    - `Client`：异步 HTTP 客户端，用于发送 HTTP 请求。
    - `ConnectionPool`：管理所有的 `Dispatcher`。
    - `Dispatcher`：管理 `Connections` 的使用权。
    - `Connector`：用于创建异步连接。
    - `Connections`：异步的 TCP 或 TLS 连接等。
  - `Util`：包含同步和异步 HTTP 客户端实现的共同组件。
    - `Redirect`：HTTP 自动重定向策略。
    - `Proxy`：HTTP 代理策略。
    - `Pool`：通用的连接池实现。
    - `OpenSSL_adapter`：OpenSSL Rust 适配层。
- `ylong_http`：提供 HTTP 基础组件的模块。
  - `Request`：HTTP 请求实现
  - `Response`：HTTP 响应实现
  - `Body`：HTTP 消息体实现，提供基础的 trait。
    - `TextBody`：纯文本的消息体实现。
    - `EmptyBody`：空的消息体实现。
    - `Mime`：Multipart 消息体实现。
    - `ylong_http`：ChunkBody 消息体实现。
  - `H1`：HTTP1 相关组件实现。
  - `H2`：HTTP2 相关组件实现。
  - `H3`：HTTP3 相关组件实现。

### ylong_http_client 库

ylong_http_client 库支持 HTTP 客户端功能，支持用户创建 HTTP 客户端向指定 Server 发送 HTTP 请求。

当前 ylong_http_client 库支持的功能：

- 同步、异步客户端
- HTTP/1.1、HTTP/2 协议版本
- 代理
- 自动重定向
- 自动重试
- 进度回调显示
- 连接管理和复用

### ylong_http 库

ylong_http 库提供了 HTTP 协议的各种基础组件，例如序列化组件、压缩组件等。

当前 ylong_http 库支持的功能：

- HTTP/1 序列化组件、HTTP/2 序列化组件
- HPACK 头部压缩实现
- Request、Response 以及相关基础类型
- Body trait 以及 Body 的各种实现

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