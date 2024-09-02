use serde::de::DeserializeOwned;
use std::env::{var, VarError};
use std::ffi::OsStr;
use std::path::Path;
use std::{fs, io};

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

pub fn from_etcd_sync<T>(client: &mut crate::etcd_client_sync::Client, key: &str, format: Format) -> ConfigResult<T>
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

    pub fn connect_sync(&self) -> Result<crate::etcd_client_sync::Client, crate::etcd_client_sync::Error> {
        crate::etcd_client_sync::Client::connect(
            &[&self.endpoint],
            self.enable_auth
                .then_some(etcd_client::ConnectOptions::new()
                    .with_user(self.user.as_ref().unwrap(), self.password.as_ref().unwrap())),
        )
    }
}
