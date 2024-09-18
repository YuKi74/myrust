#[cfg(any(
    feature = "http-jwt",
    feature = "http-tracer"
))]
pub mod serve {
    #[cfg(feature = "http-jwt")]
    pub mod jwt;
    #[cfg(feature = "http-tracer")]
    pub mod tracer;

    #[cfg(any(feature = "http-jwt"))]
    pub mod extract {
        #[cfg(feature = "http-jwt")]
        pub use super::jwt::Jwt;
    }
    #[cfg(any(feature = "http-jwt", feature = "http-tracer"))]
    pub mod middleware {
        #[cfg(feature = "http-jwt")]
        pub use super::jwt::Verifier as JwtVerifier;
        #[cfg(feature = "http-tracer")]
        pub use super::tracer::Tracer;
    }
}

#[cfg(feature = "http-request")]
pub mod request;

#[cfg(any(feature = "http-tracer", feature = "http-request"))]
pub(crate) mod trace_util;
