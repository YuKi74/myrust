pub mod config;
pub mod tracing;
pub mod id_gen;
pub mod env;
pub mod etcd_client_sync;
pub mod http;

pub use id_gen::gen_id;

use pin_project::pin_project;
#[pin_project(project=POP)]
pub(crate) enum PinOption<T> {
    Some(#[pin] T),
    None,
}
