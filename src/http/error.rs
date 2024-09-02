use axum::response::{IntoResponse, Response};
use http::StatusCode;
use tracing::{debug, error};

pub struct Error {
    desc: &'static str,
    status_code: StatusCode,
    inner: anyhow::Error,
}

impl Error {
    pub fn new(desc: &'static str, mut status_code: StatusCode, inner: impl Into<anyhow::Error>) -> Self {
        if !matches!(status_code.as_u16(), 400..600) {
            status_code = StatusCode::INTERNAL_SERVER_ERROR;
        }
        Self { desc, status_code, inner: inner.into() }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status_code: StatusCode;
        let info: String;

        if !matches!(self.status_code.as_u16(), 400..600) {
            status_code = StatusCode::INTERNAL_SERVER_ERROR;
        } else {
            status_code = self.status_code;
        }

        if status_code.is_server_error() {
            info = "INTERNAL_SERVER_ERROR".to_string();
            error!(desc=self.desc, status_code=status_code.as_u16(), err=%self.inner, "http internal server error");
        } else {
            info = format!("{}: {}", self.desc, self.inner);
            debug!(desc=self.desc, status_code=status_code.as_u16(), err=%self.inner, "http client error");
        }

        (status_code, info).into_response()
    }
}

impl<E> From<E> for Error
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            desc: "UNKNOWN_INTERNAL_ERROR",
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            inner: err.into(),
        }
    }
}

#[macro_export]
macro_rules! def_http_error {
    ($vis:vis $fn_name:ident, $desc:literal, $status_code:expr) => {
        #[inline]
        $vis fn $fn_name(err: impl Into<anyhow::Error>) -> $crate::http::error::Error {
            $crate::http::error::Error::new($desc, $status_code, err)
        }
    };

    ($vis:vis $fn_name:ident, $desc:literal) => {
        #[inline]
        $vis fn $fn_name(err: impl Into<anyhow::Error>) -> $crate::http::error::Error {
            $crate::http::error::Error::new($desc, StatusCode::INTERNAL_SERVER_ERROR, err)
        }
    };


    ($vis:vis $fn_name:ident, $status_code:expr) => {
        paste::paste! {
            #[inline]
            $vis fn $fn_name(err: impl Into<anyhow::Error>) -> $crate::http::error::Error {
                $crate::http::error::Error::new(stringify!([< $fn_name:upper >]), $status_code, err)
            }
        }
    };

    ($vis:vis $fn_name:ident) => {
        paste::paste! {
            #[inline]
            $vis fn $fn_name(err: impl Into<anyhow::Error>) -> $crate::http::error::Error {
                $crate::http::error::Error::new(stringify!([< $fn_name:upper >]), StatusCode::INTERNAL_SERVER_ERROR, err)
            }
        }
    }
}

pub mod predefined {
    use axum::http::StatusCode;

    def_http_error!(pub bad_request, StatusCode::BAD_REQUEST);
    def_http_error!(pub unauthorized, StatusCode::UNAUTHORIZED);
    def_http_error!(pub forbidden, StatusCode::FORBIDDEN);
    def_http_error!(pub not_found, StatusCode::NOT_FOUND);
    def_http_error!(pub too_many_requests, StatusCode::TOO_MANY_REQUESTS);

    def_http_error!(pub internal_error);
    def_http_error!(pub service_unavailable, StatusCode::SERVICE_UNAVAILABLE);
    def_http_error!(pub server_timeout);
    def_http_error!(pub mysql_error);
    def_http_error!(pub redis_error);
    def_http_error!(pub etcd_error);
}