# ylong_http

## Introduction

`ylong_http` has built a complete HTTP capability, supporting users to use HTTP
capability to meet the needs of communication scenarios.

`ylong_http` is written in the Rust language to support OpenHarmony's Rust
capability.

### The position of ylong_http in OpenHarmony

`ylong_http` provides HTTP protocol support to the `netstack` module in the
`OpenHarmony` system service layer, and through the `netstack` module, helps
upper layer applications build HTTP communication capabilities.

![structure](./figures/structure.png)

The following is the description information for the key fields in the figure above:

- `APP`: A direct user facing upper level application that requires the ability to upload and download.
- `request`: The component in the OpenHarmony system service layer that provides upload and download capabilities.
- `netstack`: The system component in the OpenHarmony system service layer that provides network protocol stack functionality.
- `ylong_http`: The system component in the OpenHarmony system service layer that provides HTTP protocol stack functionality.
    - `ylong_http_client`: One of the modules under `ylong_http` provides HTTP client capabilities.
    - `ylong_http`: One of the modules under `ylong_http` provides the basic components of HTTP.
- `ylong_runtime`: Rust asynchronous runtime library provided by `ylong` in the system service layer.
- `tokio`: The third-party rust asynchronous runtime library commonly used in the industry.
- `OpenSSL`: A commonly used third-party TLS implementation library in the industry.

### The internal structure of ylong_http

![inner_structure](./figures/inner_structure.png)

`ylong_http` is currently divided into two main modules: `ylong_http_client` client module and `ylong_http` protocol component module.

The `ylong_http_client` module is responsible for providing HTTP client functions, which can support users to send HTTP requests and receive HTTP responses. It is divided into three main parts:
- `sync_impl`: A synchronous HTTP client implementation that does not depend on any runtime and can run directly on the thread model, but uses a synchronous blocking strategy as a whole.
- `async_impl`: an asynchronous HTTP client implementation that requires the use of Rust's asynchronous runtime components. The asynchronous HTTP client takes advantage of Rust's asynchronous capabilities and has excellent performance.
- `Util`: The synchronous and asynchronous HTTP client parts are common, such as automatic redirection, HTTP proxy, etc.

The interface prototypes of `sync_impl` and `async_impl` are basically the same (mainly the difference between Rust asynchronous syntax and synchronous syntax), so users can switch between synchronous and asynchronous logic with a small amount of code changes.

The overall structure of `sync_impl` and `async_impl` is the same, divided into the following modules:
- `Client`: Provide the basic interface of the HTTP client externally, such as configuring related options of the client, sending HTTP requests, etc.
- `ConnectionPool`: Mainly responsible for a large number of connection management, managing the life cycle of all `Dispatcher`, including start, run, stop. The HTTP protocol is a connection-based communication protocol, involving functions such as connection multiplexing and connection management.
- `Dispatcher`: Mainly responsible for single connection management, managing the start, operation, stop, and transmission of a single connection. Each connection is governed by a `Dispatcher`, and it is up to the `Dispatcher` to determine whether the current request to be sent uses the connection it manages.
- `Connections`: connection object, which can be a TCP connection, a TLS connection or a more generalized connection object. Messages are transmitted and received on this connection, and it is the base of `Client` and the HTTP protocol.
- `Connector`: Responsible for creating connection objects. Connector is also a trait that users can use to define the behavior when creating a connection.

`Util` contains the common capabilities of synchronous and asynchronous HTTP clients, such as:
- `Redirect`: HTTP automatic redirection capability. When the HTTP response returns a status code related to redirection, the HTTP client will perform automatic redirection and automatically send a new request to the next hop.
- `Proxy`: HTTP proxy capability. When an HTTP request is sent, it is sent to a proxy instead of directly to the origin server, and the proxy server then returns the origin server's response.
- `Pool`: Universal connection pool implementation, supports the management of multiple synchronous or asynchronous connections, facilitates the reuse of existing connections by upper-layer synchronous or asynchronous clients, reduces the number of repeated connection creations, and improves performance.
- `OpenSSL_adapter`: HTTPS needs to use TLS capability on the basis of HTTP, and OpenSSL is used on OpenHarmony, so the OpenSSL interface needs to be encapsulated in Rust.

The `ylong_http` module is responsible for providing the basic capabilities of HTTP, such as HTTP2's HPACK, HTTP3's QPACK, etc. It mainly includes the following key modules:
- `Request`: The basic capability of HTTP requests, which implements all the content and behaviors of HTTP requests according to `RFC9110`. HTTP requests are mainly used to send requests to specified servers.
- `Response`: The basic capability of HTTP response, which implements all the content and behavior of HTTP response according to `RFC9110`. The HTTP response is basically the server's response to the client's request.
- `Body`:
  HTTP message body capability, according to `RFC9110` regulations to achieve all the content and behavior of the HTTP message body. The HTTP message body holds the main data content for client and server communication.
  The HTTP message body has various forms in the protocol, and there are corresponding implementations in the `ylong_http` library. For example, `EmptyBody` corresponds to an empty message body, `TextBody` corresponds to a plaintext message body, and `ChunkBody` corresponds to a chunked message body. `Mime` corresponds to the Multipart message body.
- `H1`: All basic capabilities of HTTP1, such as encoders and decoders for requests and responses in HTTP1 format.
- `H2`: All basic capabilities of HTTP2, such as encoders and decoders for requests and responses in HTTP2 format, HTTP2 frame encoders and decoders, HPACK, etc.
- `H3`: All basic capabilities of HTTP3, such as encoders and decoders for requests and responses in HTTP3 format, QPACK, etc.

## Build

`GN` is supported. User should add dependencies in `deps` of `BUILD.gn` to build this crate.

```gn
deps += ["//example_path/ylong_http_client:ylong_http_client"]
```

`Cargo` is supported. User should add dependencies in ```Cargo.toml``` to build this crate.

```toml
[dependencies]
ylong_http_client = { path = "/example_path/ylong_http_client" }
```

## Directory

```text
ylong_http
├── docs                        # User's guide
├── figures                     # Resources
├── patches                     # Patches for ci
├── ylong_http
│   ├── examples                # Examples of ylong_http
│   ├── src                     # Source code ylong_http
│   │   ├── body                # Body trait and body types
│   │   ├── h1                  # HTTP/1.1 components
│   │   ├── h2                  # HTTP/2 components
│   │   ├── h3                  # HTTP/3 components
│   │   ├── huffman             # Huffman
│   │   ├── request             # Request type
│   │   └── response            # Response type
│   └── tests                   # Tests of ylong_http
│
└── ylong_http_client
    ├── examples                # Examples of ylong_http_client
    ├── src                     # Source code of ylong_http_client
    │   ├── async_impl          # Asynchronous client implementation
    │   │   ├── conn            # Asynchronous connection layer
    │   │   ├── downloader      # Asynchronous downloader layer
    │   │   ├── ssl_stream      # Asynchronous TLS layer
    │   │   └── uploader        # Asynchronous uploader layer
    │   ├── sync_impl           # Synchronous client implementation
    │   │   └── conn            # Synchronous connection layer
    │   └── util                # Components of ylong_http_client  
    │       ├── c_openssl       # OpenSSL adapter
    │       │   ├── ffi         # OpenSSL ffi adapter
    │       │   └── ssl         # OpenSSL ssl adapter 
    │       └── config          # Configures
    │           └── tls         # TLS Configures
    │               └── alpn    # ALPN Configures
    └── tests                   # Tests of ylong_http_client
```