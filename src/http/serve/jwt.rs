use actix_web::{
    body::MessageBody,
    dev::{forward_ready, Payload, Service, ServiceRequest, ServiceResponse, Transform},
    error::InternalError,
    http::{header::{HeaderMap, ToStrError}, StatusCode},
    Error, FromRequest, HttpMessage, HttpRequest, ResponseError,
};
use async_trait::async_trait;
use jwt::{AlgorithmType, SignWithKey as _, SigningAlgorithm, ToBase64, Token, VerifyWithKey as _, VerifyingAlgorithm};
use pin_project::pin_project;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    future::{ready, Future, Ready},
    ops::Deref,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
    #[error("internal error")]
    NoVerifier,
    #[error("token not provided")]
    NotProvided,
    #[error("token is not valid string: {0}")]
    NotValidStr(#[from] ToStrError),
    #[error("invalid token type, need Bearer token")]
    InvalidTokenType,
    #[error("jwt verify error: {0}")]
    JwtError(#[from] jwt::Error),
    #[error("json deserialize error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
    #[error("token is expired")]
    Expired,
}

impl VerifierError {
    fn to_error(&self) -> Error {
        InternalError::new(
            format!("{}", self),
            self.status_code(),
        ).into()
    }
}

impl ResponseError for VerifierError {
    fn status_code(&self) -> StatusCode {
        match self {
            VerifierError::NoVerifier => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

struct ClaimsValue(Result<serde_json::Value, VerifierError>);

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
    A: VerifyingAlgorithm,
{
    pub fn new(algorithm: A, mode: VerifierMode) -> Self {
        Self { algorithm, mode }
    }
}

impl<A, S, B> Transform<S, ServiceRequest> for Verifier<A>
where
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error>,
    B: MessageBody,
    A: VerifyingAlgorithm + Clone,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = VerifierMiddleware<A, S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(VerifierMiddleware {
            algorithm: self.algorithm.clone(),
            service: Rc::new(service),
            mode: self.mode,
        }))
    }
}

pub struct VerifierMiddleware<A, S> {
    algorithm: A,
    service: Rc<S>,
    mode: VerifierMode,
}

impl<A, S, B> Service<ServiceRequest> for VerifierMiddleware<A, S>
where
    A: VerifyingAlgorithm + Clone,
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error>,
    B: MessageBody,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = VerifierFuture<A, S>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        VerifierFuture {
            algorithm: self.algorithm.clone(),
            req: Some(req),
            service: self.service.clone(),
            future: None,
            mode: self.mode,
        }
    }
}

#[pin_project]
pub struct VerifierFuture<A, S>
where
    S: Service<ServiceRequest>,
{
    algorithm: A,
    req: Option<ServiceRequest>,
    service: Rc<S>,
    #[pin]
    future: Option<S::Future>,
    mode: VerifierMode,
}

impl<A, S, B> Future for VerifierFuture<A, S>
where
    A: VerifyingAlgorithm,
    S: Service<ServiceRequest, Response=ServiceResponse<B>, Error=Error>,
    B: MessageBody,
{
    type Output = Result<ServiceResponse<B>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn verify(headers: &HeaderMap, key: &impl VerifyingAlgorithm) -> Result<serde_json::Value, VerifierError> {
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
        if this.future.is_some() {
            return this.future.as_pin_mut().unwrap().poll(cx);
        }
        match verify(this.req.as_ref().unwrap().headers(), this.algorithm) {
            Ok(v) => {
                this.req.as_mut().unwrap().extensions_mut().insert(ClaimsValue(Ok(v)));
            }
            Err(e) => {
                if matches!(this.mode, VerifierMode::MustSuccess) {
                    return Poll::Ready(Err(e.into()));
                }
                this.req.as_mut().unwrap().extensions_mut().insert(ClaimsValue(Err(e)));
            }
        }
        let future = this.service.call(this.req.take().unwrap());
        this.future.set(Some(future));
        this.future.as_pin_mut().unwrap().poll(cx)
    }
}

pub struct Jwt<T: DeserializeOwned>(pub T);
impl<T: DeserializeOwned> Deref for Jwt<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
impl<T> FromRequest for Jwt<T>
where
    T: DeserializeOwned,
{
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        fn get_jwt<T>(req: &HttpRequest) -> Result<Jwt<T>, Error>
        where
            T: DeserializeOwned,
        {
            req.extensions().get::<ClaimsValue>()
                .ok_or(VerifierError::NoVerifier)?
                .0.as_ref().map_err(|e| { e.to_error() })
                .and_then(|v| T::deserialize(v)
                    .map_err(VerifierError::from)
                    .map_err(|e| e.into()))
                .map(|t| Jwt(t))
        }
        ready(get_jwt::<T>(req))
    }
}

#[derive(Clone)]
pub struct Signer {
    key: Arc<SigningAlgorithmWrapper>,
    expiration: Duration,
}

struct SigningAlgorithmWrapper(Box<dyn SigningAlgorithm + Send + Sync + 'static>);

impl SigningAlgorithm for SigningAlgorithmWrapper {
    fn algorithm_type(&self) -> AlgorithmType {
        self.0.algorithm_type()
    }

    fn sign(&self, header: &str, claims: &str) -> Result<String, jwt::Error> {
        self.0.sign(header, claims)
    }
}

impl Signer {
    pub fn new(key: impl SigningAlgorithm + Send + Sync + 'static, expiration: Duration) -> Self {
        Self { key: Arc::new(SigningAlgorithmWrapper(Box::new(key))), expiration }
    }
    pub fn sign(&self, claims: impl ToBase64) -> Result<String, jwt::Error> {
        let header = JwtHeader {
            inner: jwt::Header {
                algorithm: self.key.algorithm_type(),
                ..Default::default()
            },
            expired_at: Time(SystemTime::now() + self.expiration),
        };
        let token = Token::new(header, claims).sign_with_key(self.key.deref())?;
        Ok(token.as_str().to_owned())
    }
}
