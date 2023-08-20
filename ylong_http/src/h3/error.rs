pub enum H3Error {
    //todo: add more
    ConnectionError(ErrorCode),
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum ErrorCode {
    QPACK_DECOMPRESSION_FAILED = 0x0200,

    QPACK_ENCODER_STREAM_ERROR = 0x0201,

    QPACK_DECODER_STREAM_ERROR = 0x0202,
}