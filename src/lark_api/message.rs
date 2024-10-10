use super::{client::{Client, CommonResp, BASE_URL}, error::{Error, Result}};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Serialize)]
pub struct SendMessageRequest {
    receive_id_type: &'static str,
    receive_id: String,
    msg_type: &'static str,
    content: String,
}

impl SendMessageRequest {
    pub fn builder() -> SendMessageRequestBuilder {
        SendMessageRequestBuilder::default()
    }
}

#[derive(Default)]
pub struct SendMessageRequestBuilder {
    receive_id_type: Option<&'static str>,
    receive_id: Option<String>,
    msg_type: Option<&'static str>,
    content: Option<String>,
}

impl SendMessageRequestBuilder {
    pub fn receiver_chat_id(mut self, chat_id: String) -> Self {
        self.receive_id_type = Some("chat_id");
        self.receive_id = Some(chat_id);
        self
    }
    pub fn receiver_email(mut self, email: String) -> Self {
        self.receive_id_type = Some("email");
        self.receive_id = Some(email);
        self
    }
    pub fn text(mut self, text: &str) -> Self {
        self.msg_type = Some("text");
        self.content = Some(serde_json::to_string(&serde_json::json!({
            "text": text,
        })).unwrap());
        self
    }
    pub fn build(self) -> Result<SendMessageRequest> {
        Ok(SendMessageRequest {
            receive_id_type: self.receive_id_type.ok_or(Error::MissingRequestParam("receive_id_type".to_string()))?,
            receive_id: self.receive_id.ok_or(Error::MissingRequestParam("receive_id".to_string()))?,
            msg_type: self.msg_type.ok_or(Error::MissingRequestParam("msg_type".to_string()))?,
            content: self.content.ok_or(Error::MissingRequestParam("content".to_string()))?,
        })
    }
}

#[derive(Deserialize)]
pub struct SendMessageResponse {
    #[serde(flatten)]
    common_resp: CommonResp,
}

impl Client {
    pub async fn send_message(&self, req: SendMessageRequest) -> Result<SendMessageResponse> {
        const URL: LazyLock<url::Url> = LazyLock::new(|| {
            BASE_URL.join("im/v1/messages").unwrap()
        });

        let token = self.get_token().await?;
        let resp = self.client.post(URL.clone())
            .header("Authorization", token)
            .query(&[("receive_id_type", req.receive_id_type)])
            .json(&req)
            .send()
            .await?;
        let resp: SendMessageResponse = resp.json().await?;
        if resp.common_resp.code != 0 {
            return Err(resp.common_resp.into());
        }
        Ok(resp)
    }
}