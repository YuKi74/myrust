use crate::{
    http::trace_util::{def_format_headers, def_tracer, TraceConfig, TRACE_ID_HEADER},
    tracing::get_trace_id,
    util::radix32::radix_32,
};
use async_trait::async_trait;
use http::{Extensions, HeaderMap, HeaderValue};
use http_body_util::BodyExt;
use hyper::body::Body;
use reqwest::{self, Request, Response};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use std::ops::Deref;
use tracing::{field::Empty, trace, trace_span, Instrument};

def_tracer!(pub Config);

struct TraceMiddleware(TraceConfig);

#[async_trait]
impl Middleware for TraceMiddleware {
    async fn handle(&self, mut req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<Response> {
        let span = trace_span!("send http request", uri=%req.url(), method=%req.method(), status=Empty);
        span.id()
            .and_then(|id| get_trace_id(&id))
            .map(|trace_id| req.headers_mut().insert(TRACE_ID_HEADER, HeaderValue::from_str(
                &format!("{}", radix_32(trace_id))).unwrap()));

        let req_headers = self.0.log_req_headers.then_some(format_headers(req.headers()));
        let req_body = (self.0.log_req_body_size > 0)
            .then_some(())
            .and_then(|_| req.body())
            .and_then(|body| body.size_hint().exact())
            .and_then(|size_hint| (size_hint <= self.0.log_req_body_size).then_some(()))
            .and_then(|_| req.body())
            .and_then(|body| body.as_bytes())
            .map(|bytes| bytes.to_owned());

        let mut resp = next.run(req, extensions).instrument(span.clone()).await?;
        span.record("status", resp.status().as_u16());

        let should_log = resp.status().is_client_error() || !self.0.only_on_error;
        let should_log_headers = should_log || self.0.always_log_headers;
        if req_headers.is_some() && should_log_headers {
            trace!(req_headers=req_headers.unwrap())
        }
        if req_body.is_some() && should_log {
            trace!(req_body=%String::from_utf8_lossy(&req_body.unwrap()))
        }
        if self.0.log_resp_headers && should_log_headers {
            trace!(resp_headers=format_headers(resp.headers()))
        }
        if self.0.log_resp_body_size > 0 && should_log &&
            resp.content_length().map(|size| size <= self.0.log_resp_body_size).unwrap_or(false) {
            let (parts, body) = http::Response::from(resp).into_parts();
            let body_bytes = body.collect()
                .await
                .map(|buf| buf.to_bytes())?;
            trace!(resp_body=%String::from_utf8_lossy(&body_bytes));
            let http_resp = http::Response::from_parts(parts, body_bytes);
            resp = Response::from(http_resp);
        }
        Ok(resp)
    }
}

def_format_headers!(HeaderMap);

#[derive(Clone)]
pub struct Client(ClientWithMiddleware);

impl Deref for Client {
    type Target = ClientWithMiddleware;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn new(config: Config) -> Client {
    from(reqwest::Client::new(), config)
}

pub fn from(client: reqwest::Client, config: Config) -> Client {
    Client(
        ClientBuilder::new(client)
            .with(TraceMiddleware(config.0))
            .build()
    )
}