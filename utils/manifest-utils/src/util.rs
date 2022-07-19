/// Tries do decode base64. On failure tries to unescape snailquotes.
pub fn decode_data<S: AsRef<str>>(input: S) -> Result<Vec<u8>, DecodingError> {
    match base64::decode(input.as_ref()) {
        Ok(data) => Ok(data),
        Err(_) => Ok(snailquote::unescape(input.as_ref()).map(String::into_bytes)?),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
    #[error("invalid input base64: {0}")]
    BlobBase64(#[from] base64::DecodeError),
    #[error("invalid escaped json string: {0}")]
    BlobJsonString(#[from] snailquote::UnescapeError),
}
