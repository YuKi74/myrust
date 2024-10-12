use crate::http::server::data::{Data, DataManager};
use actix_web::web::Json;
use actix_web::{web, Either, HttpRequest, HttpResponse, Responder, Scope};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Event {
    MessageReceive(MessageReceiveEvent),
}

#[derive(Debug)]
pub struct MessageReceiveEvent {
    pub chat_id: String,
    pub chat_type: ChatType,
    pub message: Message,
}
#[derive(Debug)]
pub enum ChatType {
    P2P,
    Group,
}
#[derive(Debug)]
pub enum Message {
    Text(String),
}

impl MessageReceiveEvent {
    fn from_raw(raw: MessageReceiveEventRaw) -> Option<Self> {
        let chat_type = match raw.message.chat_type.as_str() {
            "p2p" => ChatType::P2P,
            "group" => ChatType::Group,
            _ => return None,
        };
        let message = match raw.message.message_type.as_str() {
            "text" => {
                let text = serde_json::from_str::<TextMessage>(&raw.message.content)
                    .ok()?.text;
                Message::Text(text)
            }
            _ => return None,
        };
        Some(Self {
            chat_id: raw.message.chat_id,
            chat_type,
            message,
        })
    }
}

#[derive(Deserialize)]
struct MessageReceiveEventRaw {
    message: MessageRaw,
}
#[derive(Deserialize)]
struct MessageRaw {
    chat_id: String,
    chat_type: String,
    message_type: String,
    content: String,
}
#[derive(Deserialize)]
struct TextMessage {
    text: String,
}

#[async_trait]
pub trait Handler {
    fn verification_token(&self) -> &str;
    async fn handle(&self, event: Event);
}

pub fn handler<T>(config: &DataManager<T>) -> Scope
where
    T: Handler + 'static,
{
    web::scope("")
        .app_data(config.clone())
        .route("", web::post().to(handle::<T>))
}

#[derive(Deserialize)]
struct EventHeader {
    event_type: String,
    token: String,
}

#[derive(Deserialize)]
struct EventV2 {
    header: EventHeader,
    event: serde_json::Value,
}

#[derive(Deserialize)]
struct Challenge {
    challenge: String,
    token: String,
}

#[derive(Deserialize)]
struct EventRequest {
    #[serde(flatten)]
    v2: Option<EventV2>,
    #[serde(flatten)]
    challenge: Option<Challenge>,
}

#[derive(Serialize)]
struct EventResponse {
    challenge: String,
}

struct Empty;
impl Responder for Empty {
    type Body = &'static str;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        "".respond_to(req)
    }
}

async fn handle<T>(handler: Data<T>, req: Json<EventRequest>) -> Either<Json<EventResponse>, Empty>
where
    T: Handler + 'static,
{
    if let Some(challenge) = req.0.challenge {
        if challenge.token == handler.verification_token() {
            return Either::Left(Json(EventResponse { challenge: challenge.challenge }));
        }
        return Either::Right(Empty);
    }
    if req.v2.is_none() {
        return Either::Right(Empty);
    }

    let event = req.0.v2.unwrap();
    if event.header.token != handler.verification_token() {
        return Either::Right(Empty);
    }
    let event = parse_event(&event.header.event_type, event.event);
    if let Some(event) = event {
        actix_web::rt::spawn(async move { handler.handle(event).await });
    }
    Either::Right(Empty)
}

fn parse_event(r#type: &str, event: serde_json::Value) -> Option<Event> {
    match r#type {
        "im.message.receive_v1" => serde_json::from_value(event).ok()
            .and_then(|raw| MessageReceiveEvent::from_raw(raw))
            .map(|event| Event::MessageReceive(event)),
        _ => None,
    }
}