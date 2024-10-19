#[cfg(any(
    feature = "http-jwt",
    feature = "http-server-tracer",
    feature = "http-server-data",
    feature = "http-server-resp",
    feature = "http-server-derive",
))]
pub mod server;

#[cfg(feature = "http-client")]
pub mod client;

#[cfg(any(feature = "http-server-tracer", feature = "http-client"))]
pub(crate) mod trace_util;
