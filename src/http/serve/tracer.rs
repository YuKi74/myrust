use crate::{
    http::trace_util::{def_format_headers, def_tracer, TraceConfig, TRACE_ID_HEADER},
    util::radix32::from_radix_32,
};
use actix_web::{
    body::{self, BodySize, BoxBody, MessageBody},
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error,
    http::header::{self, HeaderMap},
    Error,
    HttpMessage,
};
use bytes::Bytes;
use pin_project::pin_project;
use std::{
    future::{ready, Future, Ready},
    pin::Pin,
    rc::Rc,
    task::{ready, Context, Poll},
};
use tracing::{error, field::Empty, trace, trace_span, Instrument, Span};

def_tracer!(pub Tracer);

impl<S, B> Transform<S, ServiceRequest> for Tracer
where
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = TracerMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TracerMiddleware {
            service: Rc::new(service),
            trace_config: self.0,
        }))
    }
}

pub struct TracerMiddleware<S> {
    trace_config: TraceConfig,
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for TracerMiddleware<S>
where
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = TracerFuture<S::Future>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let span = span_from_request(&req);
        if self.trace_config.log_req_body_size > 0 {
            TracerFuture::WithLogBody(Box::pin(with_log_body(
                req,
                self.service.clone(),
                self.trace_config,
                span.clone(),
            ).instrument(span)))
        } else {
            TracerFuture::WithoutLogBody(WithoutLogBody::new(
                req,
                self.service.clone(),
                self.trace_config,
                span,
            ))
        }
    }
}

#[pin_project(project=TFP)]
pub enum TracerFuture<Fut>
where
    Fut: Future,
{
    WithoutLogBody(#[pin] WithoutLogBody<Fut>),
    WithLogBody(#[pin] Pin<Box<dyn Future<Output=Result<ServiceResponse<BoxBody>, Error>>>>),
}

impl<Fut, B> Future for TracerFuture<Fut>
where
    Fut: Future<Output=Result<ServiceResponse<B>, Error>>,
    B: MessageBody + 'static,
{
    type Output = Result<ServiceResponse<BoxBody>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this {
            TFP::WithoutLogBody(future) => {
                Poll::Ready(ready!(future.poll(cx)))
            }
            TFP::WithLogBody(future) => {
                Poll::Ready(ready!(future.poll(cx)))
            }
        }
    }
}

#[pin_project]
pub struct WithoutLogBody<Fut> {
    span: Span,
    trace_config: TraceConfig,
    #[pin]
    future: Fut,
    req_headers: Option<String>,
}

impl<Fut, B> WithoutLogBody<Fut>
where
    Fut: Future<Output=Result<ServiceResponse<B>, Error>>,
{
    fn new<S>(req: ServiceRequest, service: S, trace_config: TraceConfig, span: Span) -> Self
    where
        S: Service<ServiceRequest, Future=Fut>,
    {
        let req_headers = trace_config.log_req_headers
            .then_some(format_headers(req.headers()));
        Self {
            span,
            trace_config,
            req_headers,
            future: service.call(req),
        }
    }
}

impl<Fut, B> Future for WithoutLogBody<Fut>
where
    Fut: Future<Output=Result<ServiceResponse<B>, Error>>,
    B: MessageBody + 'static,
{
    type Output = Result<ServiceResponse<BoxBody>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let _guard = this.span.enter();

        let result = ready!(this.future.poll(cx))
            .map(|resp| resp.map_into_boxed_body());
        match result {
            Ok(ref resp) => {
                this.span.record("status", resp.status().as_u16());
                let should_log_headers = resp.response().status().is_server_error() ||
                    !this.trace_config.only_on_error ||
                    this.trace_config.always_log_headers;

                if this.req_headers.is_some() && should_log_headers {
                    trace!(req_headers=this.req_headers.as_ref().unwrap())
                }
                if this.trace_config.log_resp_headers && should_log_headers {
                    trace!(resp_headers=format_headers(resp.headers()));
                }
                resp.response().error().map(|e| {
                    log_error(e);
                    e
                });
            }
            Err(ref e) => {
                log_error(e);
            }
        }
        Poll::Ready(result)
    }
}

fn span_from_request(req: &ServiceRequest) -> Span {
    let id = req.headers().get(TRACE_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| from_radix_32(s));
    let id = id.as_ref()
        .map(|id| id as &dyn tracing::Value)
        .unwrap_or(&Empty);
    trace_span!(
        "handle http request",
        trace_id=id,
        uri=%req.uri(),
        method=%req.method(),
        status=Empty,
    )
}

def_format_headers!(HeaderMap);

async fn with_log_body<S, B>(
    mut req: ServiceRequest,
    service: Rc<S>,
    trace_config: TraceConfig,
    span: Span,
) -> Result<ServiceResponse<BoxBody>, Error>
where
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error>,
    B: MessageBody + 'static,
{
    let req_headers = trace_config.log_req_headers
        .then_some(format_headers(req.headers()));
    let req_body = get_req_body(&mut req, trace_config.log_req_body_size).await?;

    let resp = service.call(req).await
        .map_err(|e| {
            log_error(&e);
            e
        })?;
    span.record("status", resp.status().as_u16());

    let should_log = resp.response().status().is_server_error() || !trace_config.only_on_error;
    let should_log_headers = should_log || trace_config.always_log_headers;
    if req_headers.is_some() && should_log_headers {
        trace!(req_headers=req_headers.unwrap())
    }
    if req_body.is_some() && should_log {
        trace!(req_body=%String::from_utf8_lossy(&req_body.unwrap()));
    }
    if trace_config.log_resp_headers && should_log_headers {
        trace!(resp_headers=format_headers(resp.headers()));
    }
    if trace_config.log_resp_body_size > 0 && should_log {
        return log_resp_body(resp, trace_config.log_resp_body_size).await;
    }

    Ok(resp.map_into_boxed_body())
}

async fn get_req_body(req: &mut ServiceRequest, max_size: u64) -> Result<Option<Bytes>, Error> {
    if max_size == 0 {
        return Ok(None);
    }
    if !is_text(req.headers()) {
        return Ok(None);
    }
    match content_len(req) {
        None => {
            return Ok(None)
        }
        Some(len) => {
            if len > max_size {
                trace!("request body size {} bytes exceeds maximum length of {} bytes", len, max_size);
                return Ok(None);
            }
        }
    }
    let payload = req.take_payload();
    let stream = body::BodyStream::new(payload);
    let buf = body::to_bytes(stream).await.map_err(|e| {
        let e = e.into();
        log_error(&e);
        e
    })?;
    let (_, mut payload) = actix_http::h1::Payload::create(true);
    payload.unread_data(buf.clone());
    req.set_payload(actix_web::dev::Payload::from(payload));
    Ok(Some(buf))
}

async fn log_resp_body<B>(resp: ServiceResponse<B>, max_size: u64) -> Result<ServiceResponse<BoxBody>, Error>
where
    B: MessageBody + 'static,
{
    if !is_text(resp.headers()) {
        return Ok(resp.map_into_boxed_body());
    }
    match resp.response().body().size() {
        BodySize::None => { return Ok(resp.map_into_boxed_body()); }
        BodySize::Sized(size) => {
            if size > max_size {
                trace!("request body size {} bytes exceeds maximum length of {} bytes", size, max_size);
                return Ok(resp.map_into_boxed_body());
            }
        }
        BodySize::Stream => { return Ok(resp.map_into_boxed_body()); }
    };
    let (req, resp) = resp.into_parts();
    resp.error().map(|e| {
        log_error(e);
        e
    });
    let (resp, body) = resp.into_parts();
    let body_bytes = body::to_bytes(body).await
        .map_err(|e| {
            let e = error::ErrorInternalServerError(e.into());
            log_error(&e);
            e
        })?;
    trace!(resp_body=%String::from_utf8_lossy(&body_bytes));
    let resp = resp.set_body(body_bytes.boxed());
    let resp = ServiceResponse::new(req, resp);
    Ok(resp)
}

fn log_error(e: &Error) {
    if e.as_response_error().status_code().is_server_error() {
        error!("SERVER_INTERNAL_ERROR: {:?}", e)
    } else {
        trace!("EXTERNAL_ERROR: {:?}", e)
    }
}

fn content_len(req: &ServiceRequest) -> Option<u64> {
    req.headers().get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

fn is_text(headers: &HeaderMap) -> bool {
    headers.get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s|
            s.contains(mime::APPLICATION_JSON.as_ref()) ||
                s.contains(mime::TEXT_PLAIN.as_ref()) ||
                s.contains(mime::APPLICATION_WWW_FORM_URLENCODED.as_ref())
        ).unwrap_or(false)
}
