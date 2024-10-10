use super::client::CommonResp;
use derive_builder::UninitializedFieldError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("url parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("http error: {0}")]
    HttpError(#[from] reqwest_middleware::Error),
    #[error("deserialize error: {0}")]
    DeserializeError(#[from] reqwest::Error),
    #[error("request error: {0}")]
    RequestError(#[from] CommonResp),
    #[error("invalid access token: {0}")]
    InvalidAccessToken(#[from] http::header::InvalidHeaderValue),
    #[error("missing request param: {0}")]
    MissingRequestParam(String),
}

impl From<UninitializedFieldError> for Error {
    fn from(value: UninitializedFieldError) -> Self {
        Error::MissingRequestParam(value.to_string())
    }
}

