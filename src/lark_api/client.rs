use super::error::Result;
use http::HeaderValue;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time;
use std::time::Duration;

pub const BASE_URL: LazyLock<url::Url> = LazyLock::new(|| {
    url::Url::parse("https://open.larksuite.com/open-apis/").unwrap()
});

pub struct Client {
    app_id: String,
    app_secret: String,
    pub(super) client: ClientWithMiddleware,
    token: tokio::sync::RwLock<Token>,
}

struct Token {
    token: HeaderValue,
    expired_at: time::Instant,
}

#[derive(thiserror::Error, Deserialize, Debug)]
#[error("code: {code}, msg: {msg}")]
pub struct CommonResp {
    pub code: i64,
    pub msg: String,
}

impl Client {
    pub fn new(app_id: String, app_secret: String, client: ClientWithMiddleware) -> Self {
        Self {
            app_id,
            app_secret,
            client,
            token: tokio::sync::RwLock::new(Token {
                token: HeaderValue::from_str("").unwrap(),
                expired_at: time::Instant::now(),
            }),
        }
    }

    pub(super) async fn get_token(&self) -> Result<HeaderValue> {
        const URL: LazyLock<url::Url> = LazyLock::new(|| {
            BASE_URL.join("auth/v3/tenant_access_token/internal").unwrap()
        });
        #[derive(Serialize)]
        struct Request<'a> {
            app_id: &'a str,
            app_secret: &'a str,
        }
        #[derive(Deserialize)]
        struct Response {
            #[serde(flatten)]
            common_resp: CommonResp,
            tenant_access_token: String,
            expire: u64,
        }

        let t = self.token.read().await;
        if t.expired_at.elapsed().is_zero() {
            return Ok(t.token.clone());
        }
        drop(t);
        let mut t = self.token.write().await;
        if t.expired_at.elapsed().is_zero() {
            return Ok(t.token.clone());
        }

        let resp = self.client.post(URL.clone())
            .json(&Request {
                app_id: &self.app_id,
                app_secret: &self.app_secret,
            })
            .send()
            .await?;
        let resp: Response = resp.json().await?;
        if resp.common_resp.code != 0 {
            return Err(resp.common_resp.into());
        }

        t.token = HeaderValue::from_str(&format!("Bearer {}", resp.tenant_access_token))?;
        t.expired_at = time::Instant::now() + Duration::from_secs(resp.expire.checked_sub(60).unwrap_or(0));
        Ok(t.token.clone())
    }
}
