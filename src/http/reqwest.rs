use super::tracer::TRACE_ID_HEADER;
use crate::tracing::get_trace_id;
use async_trait::async_trait;
use http::{Extensions, HeaderValue};
use reqwest::{self, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use std::ops::Deref;
use tracing::Span;

struct TraceMiddleware;

#[async_trait]
impl Middleware for TraceMiddleware {
    async fn handle(&self, mut req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<Response> {
        Span::current().id()
            .and_then(|id| get_trace_id(&id))
            .map(|trace_id| req.headers_mut().insert(TRACE_ID_HEADER, HeaderValue::from_str(
                &format!("{:x}", trace_id)).unwrap()));
        next.run(req, extensions).await
    }
}

#[derive(Clone)]
pub struct Client(ClientWithMiddleware);

impl Deref for Client {
    type Target = ClientWithMiddleware;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn new() -> Client {
    from(reqwest::Client::new())
}

pub fn from(client: reqwest::Client) -> Client {
    Client(
        ClientBuilder::new(client)
            .with(TraceMiddleware)
            .build()
    )
}