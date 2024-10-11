use etcd_client::EventType;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use std::{
    env::{var, VarError},
    ffi::OsStr, fs, io, path::Path, pin::Pin,
    task::{ready, Context, Poll},
};

#[derive(Clone, Copy)]
pub enum Format {
    Json,
    Yaml,
    Toml,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unknown format")]
    UnknownFormat,
    #[error("unsupported format: '{0}'")]
    UnsupportedFormat(String),

    #[error("deserialize json error occurred")]
    DeserializeJsonError(#[from] serde_json::Error),
    #[error("deserialize yaml error occurred")]
    DeserializeYamlError(#[from]serde_yaml::Error),
    #[error("deserialize toml error occurred")]
    DeserializeTomlError(#[from] toml::de::Error),

    #[error("io error occurred")]
    IoError(#[from] io::Error),

    #[error("etcd client error occurred")]
    EtcdClientError(#[from]etcd_client::Error),
    #[error("etcd key: '{0}' not exists")]
    EtcdKeyNotExists(String),
}

type ConfigResult<T> = Result<T, Error>;

fn deserialize<T>(format: Format, buf: &str) -> ConfigResult<T>
where
    T: DeserializeOwned,
{
    Ok(match format {
        Format::Json => serde_json::from_str(buf)?,
        Format::Yaml => serde_yaml::from_str(buf)?,
        Format::Toml => toml::de::from_str(buf)?,
    })
}

pub fn from_file<T>(path: impl AsRef<Path>, format: Format) -> ConfigResult<T>
where
    T: DeserializeOwned,
{
    let buf = fs::read_to_string(path)?;

    deserialize(format, &buf)
}

pub fn from_file_auto<T>(path: impl AsRef<Path>) -> ConfigResult<T>
where
    T: DeserializeOwned,
{
    path.as_ref().extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            match ext {
                "json" => Ok(Format::Json),
                "yaml" => Ok(Format::Yaml),
                "toml" => Ok(Format::Toml),
                _ => Err(Error::UnsupportedFormat(ext.to_string())),
            }
        })
        .ok_or(Error::UnknownFormat)?
        .map(|format| {
            from_file(path, format)
        })?
}

pub async fn from_etcd<T>(client: &mut etcd_client::Client, key: &str, format: Format) -> ConfigResult<T>
where
    T: DeserializeOwned,
{
    let resp = client.get(key, None).await?;
    let kvs = resp.kvs();
    if kvs.len() == 0 {
        return Err(Error::EtcdKeyNotExists(key.to_string()));
    }

    let buf = kvs[0].value_str()?;
    deserialize(format, buf)
}

#[cfg(feature = "etcd-client-sync")]
pub fn from_etcd_sync<T>(client: &mut super::etcd_client_sync::Client, key: &str, format: Format) -> ConfigResult<T>
where
    T: DeserializeOwned,
{
    let resp = client.get(key, None)?;
    let kvs = resp.kvs();
    if kvs.len() == 0 {
        return Err(Error::EtcdKeyNotExists(key.to_string()));
    }

    let buf = kvs[0].value_str()?;
    deserialize(format, buf)
}

pub struct EtcdConfig {
    endpoint: String,
    enable_auth: bool,
    user: Option<String>,
    password: Option<String>,
}

impl EtcdConfig {
    pub fn from_env() -> Result<Self, VarError> {
        let endpoint = var("ETCD_ENDPOINT")?;
        let enable_auth = var("ETCD_ENABLE_AUTH")
            .is_ok_and(|s| {
                !matches!(s.as_str(), "false"|"FALSE"|"False"|"no"|"No"|"NO"|"0")
            });

        let user = enable_auth.then_some(var("ETCD_USER")?);
        let password = enable_auth.then_some(var("ETCD_PASSWORD")?);

        Ok(Self {
            endpoint,
            enable_auth,
            user,
            password,
        })
    }

    pub fn endpoint(&self) -> &String {
        &self.endpoint
    }

    pub fn enable_auth(&self) -> bool {
        self.enable_auth
    }

    pub fn user(&self) -> Option<&String> {
        self.user.as_ref()
    }

    pub fn password(&self) -> Option<&String> {
        self.password.as_ref()
    }

    pub async fn connect(&self) -> Result<etcd_client::Client, etcd_client::Error> {
        etcd_client::Client::connect(
            &[&self.endpoint],
            self.enable_auth
                .then_some(etcd_client::ConnectOptions::new()
                    .with_user(self.user.as_ref().unwrap(), self.password.as_ref().unwrap())),
        ).await
    }

    #[cfg(feature = "etcd-client-sync")]
    pub fn connect_sync(&self) -> Result<super::etcd_client_sync::Client, super::etcd_client_sync::Error> {
        super::etcd_client_sync::Client::connect(
            &[&self.endpoint],
            self.enable_auth
                .then_some(etcd_client::ConnectOptions::new()
                    .with_user(self.user.as_ref().unwrap(), self.password.as_ref().unwrap())),
        )
    }
}

pub struct EtcdConfigWatcher(etcd_client::Watcher);

#[pin_project]
pub struct EtcdConfigWatcherStream<T: DeserializeOwned> {
    #[pin]
    watch_stream: etcd_client::WatchStream,
    format: Format,
    items: Vec<ConfigResult<T>>,
}

pub async fn watch_etcd<T>(
    client: &mut etcd_client::Client,
    key: &str,
    format: Format,
) -> Result<(EtcdConfigWatcher, EtcdConfigWatcherStream<T>), etcd_client::Error>
where
    T: DeserializeOwned,
{
    let (watcher, watch_stream) = client.watch(key, None).await?;
    Ok((EtcdConfigWatcher(watcher), EtcdConfigWatcherStream {
        watch_stream,
        format,
        items: vec![],
    }))
}

impl EtcdConfigWatcher {
    pub async fn cancel(&mut self) -> Result<(), etcd_client::Error> {
        self.0.cancel().await
    }
}

impl<T: DeserializeOwned> futures::Stream for EtcdConfigWatcherStream<T> {
    type Item = ConfigResult<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if this.items.len() > 0 {
            return Poll::Ready(Some(this.items.remove(0)));
        }

        let msg = ready!(this.watch_stream.poll_next(cx));
        if msg.is_none() {
            return Poll::Ready(None);
        }
        let msg = msg.unwrap();
        if msg.is_err() {
            return Poll::Ready(Some(Err(Error::EtcdClientError(msg.unwrap_err()))));
        }
        let resp = msg.unwrap();
        if resp.canceled() {
            return Poll::Ready(None);
        }

        for event in resp.events() {
            match event.event_type() {
                EventType::Delete => {}

                EventType::Put => {
                    event.kv().map(|kv| {
                        kv.value_str().map(|buf| {
                            this.items.push(deserialize(*this.format, buf))
                        })
                    });
                }
            }
        }

        if this.items.len() > 0 {
            Poll::Ready(Some(this.items.remove(0)))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
