use crate::{PinOption, POP};
use axum::{
    body::Body,
    extract::Request,
    response::{IntoResponse, Response},
};
use base64::Engine;
use http::{header, HeaderMap};
use pin_project::pin_project;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    str::from_utf8,
    task::{ready, Context, Poll},
};
use tower::{Layer, Service};
use tracing::{field::Empty, trace, trace_span, Span, Value};

pub(crate) const TRACE_ID_HEADER: &str = "X-Trace-Id";

#[derive(Clone, Default)]
pub struct Tracer {
    log_req_headers: bool,
    log_resp_headers: bool,
    log_req_body_size: usize,
    log_resp_body_size: usize,
}

pub struct TracerBuilder(Tracer);

impl Tracer {
    pub fn builder() -> TracerBuilder {
        TracerBuilder(Self::default())
    }
}

impl TracerBuilder {
    pub fn with_log_req_headers(self, log_req_headers: bool) -> Self {
        TracerBuilder(Tracer { log_req_headers, ..self.0 })
    }
    pub fn with_log_resp_headers(self, log_resp_headers: bool) -> Self {
        TracerBuilder(Tracer { log_resp_headers, ..self.0 })
    }
    pub fn with_log_req_body_size(self, log_req_body_size: usize) -> Self {
        TracerBuilder(Tracer { log_req_body_size, ..self.0 })
    }
    pub fn with_log_resp_body_size(self, log_resp_body_size: usize) -> Self {
        TracerBuilder(Tracer { log_resp_body_size, ..self.0 })
    }
    pub fn with_log_headers(self, log_headers: bool) -> Self {
        TracerBuilder(Tracer {
            log_req_headers: log_headers,
            log_resp_headers: log_headers,
            ..self.0
        })
    }
    pub fn with_log_body_size(self, log_body_size: usize) -> Self {
        TracerBuilder(Tracer {
            log_req_body_size: log_body_size,
            log_resp_body_size: log_body_size,
            ..self.0
        })
    }
    pub fn build(self) -> Tracer {
        self.0
    }
}

impl<I> Layer<I> for Tracer {
    type Service = TracerService<I>;

    fn layer(&self, inner: I) -> Self::Service {
        TracerService {
            tracer: self.clone(),
            inner,
        }
    }
}

#[derive(Clone)]
pub struct TracerService<I> {
    tracer: Tracer,
    inner: I,
}

impl<I> Service<Request> for TracerService<I>
where
    I: Service<Request, Response=Response, Error=Infallible> + Clone + Send + 'static,
    I::Future: Send,
{
    type Response = I::Response;
    type Error = I::Error;
    type Future = TraceFuture<I>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let trace_id = req.headers().get(TRACE_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| u128::from_str_radix(s, 16).ok());
        let t = trace_id.as_ref().map(|v| v as &dyn Value).unwrap_or(&Empty);
        TraceFuture {
            inner: self.inner.clone(),
            span: trace_span!("http request", trace_id=t, uri=%req.uri(), method=%req.method(), status=Empty),
            req: Some(req),
            tracer: self.tracer.clone(),
            status: TraceFutureStatus::Beginning,
            future: PinOption::None,
            inner_future: PinOption::None,
        }
    }
}

#[pin_project]
pub struct TraceFuture<I>
where
    I: Service<Request, Response=Response, Error=Infallible>,
{
    inner: I,
    req: Option<Request>,
    span: Span,
    tracer: Tracer,
    status: TraceFutureStatus,
    #[pin]
    future: PinOption<Pin<Box<dyn Future<Output=Result<Response, Infallible>> + Send>>>,
    #[pin]
    inner_future: PinOption<I::Future>,
}
enum TraceFutureStatus {
    Beginning,
    PollingFuture,
    PollingInner,
}

impl<I> Future for TraceFuture<I>
where
    I: Service<Request, Response=Response, Error=Infallible> + Clone + Send + 'static,
    I::Future: Send,
{
    type Output = Result<Response, Infallible>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let _guard = this.span.enter();

        if matches!(this.status, TraceFutureStatus::PollingFuture) {
            if let POP::Some(future) = this.future.project() {
                return future.poll(cx);
            } else {
                panic!("should never happened")
            }
        }


        if matches!(this.status, TraceFutureStatus::PollingInner) {
            if let POP::Some(inner_future) = this.inner_future.project() {
                let resp = ready!(inner_future.poll(cx)).unwrap();
                if this.tracer.log_resp_headers {
                    trace!(resp_headers=?resp.headers());
                }
                return Poll::Ready(Ok(resp));
            } else {
                panic!("should never happened")
            }
        }

        if this.tracer.log_req_body_size == 0 && this.tracer.log_resp_body_size == 0 {
            if this.tracer.log_req_headers {
                trace!(req_headers=?this.req.as_ref().unwrap().headers());
            }
            this.inner_future.set(PinOption::Some(this.inner.call(this.req.take().unwrap())));
            *this.status = TraceFutureStatus::PollingInner;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let tracer = this.tracer.clone();
        let mut req = this.req.take().unwrap();
        let mut inner = this.inner.clone();
        let span = this.span.clone();

        this.future.set(PinOption::Some(Box::pin(async move {
            use super::error::predefined::internal_error;

            if tracer.log_req_headers {
                trace!(req_headers=?req.headers());
            }

            if tracer.log_req_body_size > 0 {
                let (parts, body) = req.into_parts();
                let future = log_body(
                    BodyType::Request, &parts.headers, body,
                    tracer.log_req_body_size);
                let result = future.await;
                match result {
                    Ok(body) => {
                        req = Request::from_parts(parts, body);
                    }
                    Err(e) => {
                        return Ok(internal_error(e).into_response())
                    }
                }
            }

            let future = inner.call(req);
            let mut resp = future.await.unwrap();
            span.record("status", resp.status().as_u16());
            if tracer.log_resp_headers {
                trace!(resp_headers=?resp.headers());
            }

            if tracer.log_resp_body_size > 0 {
                let (parts, body) = resp.into_parts();
                let future = log_body(
                    BodyType::Response, &parts.headers, body,
                    tracer.log_resp_body_size);
                let result = future.await;
                match result {
                    Ok(body) => {
                        resp = Response::from_parts(parts, body);
                    }
                    Err(e) => {
                        return Ok(internal_error(e).into_response())
                    }
                }
            }

            Ok(resp)
        })));

        *this.status = TraceFutureStatus::PollingFuture;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

enum BodyType {
    Request,
    Response,
}
impl BodyType {
    fn log(&self, content: &str) {
        match self {
            BodyType::Request => {
                trace!(req_body=content)
            }
            BodyType::Response => {
                trace!(resp_body=content)
            }
        }
    }
}
async fn log_body(body_type: BodyType, headers: &HeaderMap, body: Body, max_body_size: usize) -> Result<Body, axum::Error> {
    let length = headers.get(header::CONTENT_LENGTH)
        .map(|v| {
            v.to_str().unwrap_or("0")
                .parse::<usize>().unwrap_or(0)
        })
        .unwrap_or(0);
    if length > max_body_size {
        let content = format!("body exceeded maximum length of {}, skip logging", max_body_size);
        body_type.log(&content);
        return Ok(body);
    }
    let body_bytes = axum::body::to_bytes(body, length).await?;
    if let Ok(content) = from_utf8(body_bytes.as_ref()) {
        body_type.log(content);
    } else {
        let content = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&body_bytes);
        body_type.log(&content);
    }
    Ok(Body::from(body_bytes))
}