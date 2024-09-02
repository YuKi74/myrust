use axum::{routing::{get, post}, Json, Router};
use hmac::{Hmac, Mac};
use myrust::http::extract::Jwt;
use myrust::http::jwt::VerifierMode;
use myrust::http::middleware::Tracer;
use myrust::http::{self, jwt::{Signer, Verifier}};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    myrust::tracing::setup_cloud_native("debug,reqwest=off,hyper_util=off");

    let hmac = Hmac::<Sha256>::new_from_slice(b"my-secret")?;
    let verifier = Verifier::new(hmac.clone(), VerifierMode::AllowFailed);
    let signer = Signer::new(hmac, Duration::from_secs(3600));
    let router = Router::new()
        .route("/", get(handle_get).layer(verifier))
        .route("/", post(login).layer(signer))
        .layer(Tracer::builder()
            // .with_log_headers(true)
            // .with_log_body_size(1024 * 1024 * 10)
            .build());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct JwtPayload {
    username: String,
    gender: String,
}

async fn handle_get(Jwt(payload): Jwt<JwtPayload>) -> http::Result<()> {
    println!("{:?}", payload);
    Ok(())
}

async fn login() -> http::Result<Json<JwtPayload>> {
    Ok(Json(JwtPayload {
        username: "li hua".to_string(),
        gender: "female".to_string(),
    }))
}