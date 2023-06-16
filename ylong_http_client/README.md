# ylong_http_client

### 简介

ylong_http_client 支持用户构建 HTTP 客户端，支持用户使用该客户端向服务器发送请求，接收并解析服务器返回的响应。

##### Client

ylong_http_client 支持用户创建同步或者异步 HTTP 客户端，用户可以使用 mod 区分两种客户端。

- `sync_impl::Client`：同步 HTTP 客户端，整体流程使用同步接口。

- `async_impl::Client`：异步 HTTP 客户端，整体流程使用异步接口。

不论是同步还是异步客户端，都具有相同的功能，例如：连接复用、自动重定向、自动重试、设置代理等功能。

ylong_http_client 创建的客户端支持以下 HTTP 版本：

- `HTTP/1.1`

- `HTTP/2`

- `HTTP/3`

##### Request 和 Response

ylong_http_client 使用 ylong_http 库提供的 `Request` 结构，支持用户自定义请求内容。

在使用客户端发送完请求后，接收到的响应会以 ylong_http 库提供的 `Response` + `HttpBody` 的结构返回。

用户可以使用 `Response` 提供的接口来获取请求信息，并且可以使用 ylong_http 提供的 `Body` trait 读取响应的内容。用户也可以使用 ylong_http_client 提供的 `BodyReader` 读取内容。

### 编译构建

在 ```Cargo.toml``` 下添加依赖。添加后使用 ```cargo``` 进行编译和构建：

```toml
[dependencies]
ylong_http_client = "1.9.0"
```