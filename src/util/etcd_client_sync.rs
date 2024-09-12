use etcd_client::{GetOptions, GetResponse};
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error occurred")]
    IoError(#[from]std::io::Error),
    #[error("etcd client error occurred")]
    EtcdClientError(#[from]etcd_client::Error),
}

#[derive(Clone)]
pub struct Client {
    inner: etcd_client::Client,
    rt: Arc<tokio::runtime::Runtime>,
}

type EtcdResult<T> = Result<T, etcd_client::Error>;

impl Client {
    pub fn new(inner: etcd_client::Client) -> Result<Self, Error> {
        Ok(Self {
            inner,
            rt: Arc::new(tokio::runtime::Builder::new_current_thread().enable_all().build()?),
        })
    }

    pub fn into_inner(self) -> etcd_client::Client {
        self.inner
    }

    pub fn connect<E, S>(endpoints: S, options: Option<etcd_client::ConnectOptions>) -> Result<Self, Error>
    where
        E: AsRef<str>,
        S: AsRef<[E]>,
    {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let rt = Arc::new(rt);
        let inner = rt.block_on(
            etcd_client::Client::connect(endpoints, options))?;

        Ok(Self {
            inner,
            rt,
        })
    }

    pub fn get(&mut self, key: impl Into<Vec<u8>>, options: Option<GetOptions>) -> EtcdResult<GetResponse> {
        self.rt.block_on(self.inner.get(key, options))
    }
}