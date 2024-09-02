use crate::{PinOption, POP};
use anyhow::anyhow;
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{FromRequestParts, Request},
    response::{IntoResponse, Response},
};
use http::{self, request, StatusCode};
use jwt::{SignWithKey as _, SigningAlgorithm, VerifyWithKey as _, VerifyingAlgorithm};
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    future::Future,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower::{Layer, Service};

struct Time(SystemTime);
impl Serialize for Time {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let timestamp = self.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        serializer.serialize_u64(timestamp)
    }
}
impl<'de> Deserialize<'de> for Time
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let timestamp = u64::deserialize(deserializer)?;
        Ok(Self(UNIX_EPOCH + Duration::from_millis(timestamp)))
    }
}
impl Deref for Time {
    type Target = SystemTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Serialize, Deserialize)]
pub struct JwtHeader {
    #[serde(flatten)]
    inner: jwt::Header,
    expired_at: Time,
}

impl jwt::JoseHeader for JwtHeader {
    fn algorithm_type(&self) -> jwt::AlgorithmType {
        self.inner.algorithm_type()
    }
    fn key_id(&self) -> Option<&str> {
        self.inner.key_id()
    }
    fn type_(&self) -> Option<jwt::header::HeaderType> {
        self.inner.type_()
    }
    fn content_type(&self) -> Option<jwt::header::HeaderContentType> {
        self.inner.content_type()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum VerifierError {
    #[error("token not provided")]
    NotProvided,
    #[error("token is not valid string")]
    NotValidStr(#[from] http::header::ToStrError),
    #[error("invalid token type, need Bearer token")]
    InvalidTokenType,
    #[error("jwt verify error")]
    JwtError(#[from] jwt::Error),
    #[error("token is expired")]
    Expired,
}

impl IntoResponse for VerifierError {
    fn into_response(self) -> Response {
        use super::error::predefined::unauthorized;
        match self {
            VerifierError::NotProvided |
            VerifierError::InvalidTokenType |
            VerifierError::Expired => {
                unauthorized(self).into_response()
            }
            VerifierError::NotValidStr(e) => {
                unauthorized(e).into_response()
            }
            VerifierError::JwtError(e) => {
                unauthorized(e).into_response()
            }
        }
    }
}

#[derive(Clone)]
pub enum VerifierErrorErased {
    NotProvided,
    NotValidStr,
    InvalidTokenType,
    JwtError(String),
    Expired,
}

impl From<VerifierError> for VerifierErrorErased {
    fn from(value: VerifierError) -> Self {
        match value {
            VerifierError::NotProvided => {
                Self::NotProvided
            }
            VerifierError::NotValidStr(_) => {
                Self::NotValidStr
            }
            VerifierError::InvalidTokenType => {
                Self::InvalidTokenType
            }
            VerifierError::JwtError(e) => {
                Self::JwtError(e.to_string())
            }
            VerifierError::Expired => {
                Self::Expired
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Rejection {
    #[error("no verifier")]
    NoVerifier,
    #[error("token not provided")]
    NotProvided,
    #[error("token is not valid string")]
    NotValidStr,
    #[error("invalid token type, need Bearer token")]
    InvalidTokenType,
    #[error("verify failed, invalid jwt token")]
    JwtError(String),
    #[error("token is expired")]
    Expired,
    #[error("invalid json payload")]
    SerdeJsonError(#[from] serde_json::Error),
}

impl From<VerifierErrorErased> for Rejection {
    fn from(value: VerifierErrorErased) -> Self {
        match value {
            VerifierErrorErased::NotProvided => {
                Self::NotProvided
            }
            VerifierErrorErased::NotValidStr => {
                Self::NotValidStr
            }
            VerifierErrorErased::InvalidTokenType => {
                Self::InvalidTokenType
            }
            VerifierErrorErased::JwtError(e) => {
                Self::JwtError(e)
            }
            VerifierErrorErased::Expired => {
                Self::Expired
            }
        }
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        use super::error::predefined::{internal_error, unauthorized};
        match self {
            Rejection::NoVerifier => {
                internal_error(self).into_response()
            }
            Rejection::SerdeJsonError(e) => {
                unauthorized(e).into_response()
            }
            _ => {
                unauthorized(self).into_response()
            }
        }
    }
}

#[derive(Clone)]
struct ClaimsValue(Arc<Result<serde_json::Value, VerifierErrorErased>>);

#[derive(Clone, Copy)]
pub enum VerifierMode {
    MustSuccess,
    AllowFailed,
}
#[derive(Clone)]
pub struct Verifier<A> {
    algorithm: A,
    mode: VerifierMode,
}

impl<A> Verifier<A>
where
    A: VerifyingAlgorithm + Clone + Send + Sync + 'static,
{
    pub fn new(algorithm: A, mode: VerifierMode) -> Self {
        Self { algorithm, mode }
    }
}

impl<A, I> Layer<I> for Verifier<A>
where
    A: Clone,
{
    type Service = VerifierService<A, I>;

    fn layer(&self, inner: I) -> Self::Service {
        VerifierService { algorithm: self.algorithm.clone(), inner, mode: self.mode }
    }
}

#[derive(Clone)]
pub struct VerifierService<A, I> {
    algorithm: A,
    inner: I,
    mode: VerifierMode,
}

impl<A, I> Service<Request> for VerifierService<A, I>
where
    A: VerifyingAlgorithm + Clone,
    I: Service<Request, Response=Response> + Clone,
{
    type Response = Response;
    type Error = I::Error;
    type Future = VerifierFuture<A, I, I::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        VerifierFuture {
            algorithm: self.algorithm.clone(),
            inner: self.inner.clone(),
            req: Some(req),
            future: PinOption::None,
            mode: self.mode,
        }
    }
}

#[pin_project]
pub struct VerifierFuture<A, I, Fut> {
    algorithm: A,
    inner: I,
    req: Option<Request>,
    #[pin]
    future: PinOption<Fut>,
    mode: VerifierMode,
}

impl<A, I> Future for VerifierFuture<A, I, I::Future>
where
    A: VerifyingAlgorithm,
    I: Service<Request, Response=Response>,
{
    type Output = Result<Response, I::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn verify(headers: &http::HeaderMap, key: &impl VerifyingAlgorithm) -> Result<serde_json::Value, VerifierError> {
            let token = headers.get("Authorization")
                .ok_or(VerifierError::NotProvided)?
                .to_str()?
                .strip_prefix("Bearer ")
                .ok_or(VerifierError::InvalidTokenType)?;

            let token: jwt::Token<JwtHeader, serde_json::Value, jwt::Verified> =
                token.verify_with_key(key)?;
            let (header, claims) = token.into();
            if header.expired_at.lt(&SystemTime::now()) {
                return Err(VerifierError::Expired);
            }
            Ok(claims)
        }

        let mut this = self.project();
        if let POP::Some(future) = this.future.as_mut().project() {
            return future.poll(cx);
        }
        match verify(this.req.as_ref().unwrap().headers(), this.algorithm) {
            Ok(v) => {
                this.req.as_mut().unwrap().extensions_mut().insert(ClaimsValue(Arc::new(Ok(v))));
            }
            Err(e) => {
                if matches!(this.mode, VerifierMode::MustSuccess) {
                    return Poll::Ready(Ok(e.into_response()));
                }
                this.req.as_mut().unwrap().extensions_mut().insert(ClaimsValue(Arc::new(Err(e.into()))));
            }
        }
        let future = this.inner.call(this.req.take().unwrap());
        this.future.set(PinOption::Some(future));
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

pub struct Jwt<T: DeserializeOwned>(pub T);

#[async_trait]
impl<S, T> FromRequestParts<S> for Jwt<T>
where
    T: DeserializeOwned,
{
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut request::Parts, _: &S) -> Result<Self, Self::Rejection> {
        let value =
            parts.extensions.get::<ClaimsValue>()
                .ok_or(Rejection::NoVerifier)?
                .0.deref().as_ref();
        match value {
            Ok(value) => {
                Ok(Jwt(T::deserialize(value)?))
            }
            Err(e) => {
                Err(e.clone().into())
            }
        }
    }
}

#[derive(Serialize, Clone)]
pub struct SignResp {
    pub token: String,
}
impl From<String> for SignResp {
    fn from(token: String) -> Self {
        Self { token }
    }
}

#[derive(Clone)]
pub struct Signer<A, R = SignResp> {
    algorithm: A,
    expiration: Duration,
    r: PhantomData<R>,
}
impl<A> Signer<A, SignResp>
where
    A: SigningAlgorithm,
{
    pub fn new(algorithm: A, expiration: Duration) -> Self {
        Self { algorithm, expiration, r: PhantomData }
    }
    pub fn new_with_custom_resp<R1>(algorithm: A, expiration: Duration) -> Signer<A, R1>
    where
        R1: From<String> + Serialize + Clone,
    {
        Signer { algorithm, expiration, r: PhantomData }
    }
}
impl<A, R> Signer<A, R>
where
    A: SigningAlgorithm,
{
    pub fn with_custom_resp<R1>(self) -> Signer<A, R1>
    where
        R1: From<String> + Serialize + Clone,
    {
        Signer { algorithm: self.algorithm, expiration: self.expiration, r: PhantomData }
    }
}
impl<A, R, I> Layer<I> for Signer<A, R>
where
    A: Clone,
{
    type Service = SignerService<A, R, I>;

    fn layer(&self, inner: I) -> Self::Service {
        SignerService {
            algorithm: self.algorithm.clone(),
            expiration: self.expiration,
            inner,
            r: PhantomData,
        }
    }
}
#[derive(Clone)]
pub struct SignerService<A, R, I> {
    algorithm: A,
    expiration: Duration,
    inner: I,
    r: PhantomData<R>,
}
impl<A, R, I> Service<Request> for SignerService<A, R, I>
where
    A: SigningAlgorithm + Clone + Send + 'static,
    R: From<String> + Serialize,
    I: Service<Request, Response=Response>,
    I::Future: Send + 'static,
{
    type Response = I::Response;
    type Error = I::Error;
    type Future = Pin<Box<dyn Future<Output=Result<Response, I::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        use super::error::predefined::internal_error;

        let future = self.inner.call(req);
        let algorithm = self.algorithm.clone();
        let expiration = self.expiration;

        Box::pin(async move {
            let resp = future.await?;
            if resp.status().as_u16() != StatusCode::OK {
                return Ok(resp);
            }
            let (mut parts, body) = resp.into_parts();
            let read_body_result = axum::body::to_bytes(body, usize::MAX).await;
            if let Err(e) = read_body_result {
                return Ok(internal_error(e).into_response());
            }
            let body_bytes = read_body_result.unwrap();
            let jwt_header = JwtHeader {
                inner: jwt::Header {
                    algorithm: algorithm.algorithm_type(),
                    ..Default::default()
                },
                expired_at: Time(SystemTime::now() + expiration),
            };
            let deserialize_result = serde_json::from_slice::<serde_json::Value>(body_bytes.as_ref());
            if let Err(e) = deserialize_result {
                return Ok(internal_error(anyhow!("not valid json payload: {}", e)).into_response());
            }
            let token = jwt::Token::new(jwt_header, deserialize_result.unwrap());
            let sign_result = token.sign_with_key(&algorithm);
            if let Err(e) = sign_result {
                return Ok(internal_error(e).into_response());
            }
            let return_value = R::from(sign_result.unwrap().as_str().to_string());
            let serialize_result = serde_json::to_vec(&return_value);
            if let Err(e) = serialize_result {
                return Ok(internal_error(e).into_response());
            }
            parts.headers.remove(http::header::CONTENT_LENGTH);
            parts.headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static(mime::APPLICATION_JSON.as_ref()),
            );
            Ok(Response::from_parts(parts, Body::from(serialize_result.unwrap())))
        })
    }
}