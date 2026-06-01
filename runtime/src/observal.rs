//! Capsule Runtime - Observal admin API client

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum ObservalError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Observal API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("Observal not configured")]
    NotConfigured,
}

#[derive(Debug, Clone)]
pub struct ObservalTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
struct CreateUserRequest {
    email: String,
    name: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct CreateUserResponse {
    #[allow(dead_code)]
    id: String,
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: String,
}

pub struct ObservalClient {
    api_url: String,
    container_api_url: String, // URL containers use to reach Observal (host.docker.internal)
    admin_email: String,
    admin_password: String,
    admin_token: Option<String>,
    http: Client,
}

impl ObservalClient {
    pub fn from_env() -> Option<Self> {
        let api_url = std::env::var("OBSERVAL_API_URL").ok()?;
        // URL that containers use to reach Observal — on Docker Desktop, host.docker.internal
        let container_api_url = std::env::var("OBSERVAL_CONTAINER_URL")
            .unwrap_or_else(|_| api_url.replace("127.0.0.1", "host.docker.internal"));
        let admin_email =
            std::env::var("OBSERVAL_ADMIN_EMAIL").unwrap_or_else(|_| "admin@capsule.local".into());
        let admin_password = std::env::var("OBSERVAL_ADMIN_PASSWORD")
            .unwrap_or_else(|_| "capsule-admin-changeme".into());

        Some(Self {
            api_url,
            container_api_url,
            admin_email,
            admin_password,
            admin_token: None,
            http: Client::new(),
        })
    }

    /// Authenticate as admin and cache the token
    pub async fn authenticate_admin(&mut self) -> Result<(), ObservalError> {
        let resp = self
            .http
            .post(format!("{}/api/v1/auth/login", self.api_url))
            .json(&LoginRequest {
                email: self.admin_email.clone(),
                password: self.admin_password.clone(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ObservalError::Api { status, body });
        }

        let login: LoginResponse = resp.json().await?;
        self.admin_token = Some(login.access_token);
        info!("Observal admin authenticated");
        Ok(())
    }

    fn admin_token(&self) -> Result<&str, ObservalError> {
        self.admin_token
            .as_deref()
            .ok_or(ObservalError::NotConfigured)
    }

    /// Create a new Observal user for a session. Returns (email, password).
    pub async fn create_user(&self, session_id: &str) -> Result<(String, String), ObservalError> {
        let token = self.admin_token()?;
        let email = format!("session-{}@capsule.local", &session_id[..8]);

        let resp = self
            .http
            .post(format!("{}/api/v1/admin/users", self.api_url))
            .bearer_auth(token)
            .json(&CreateUserRequest {
                email: email.clone(),
                name: format!("Session {}", &session_id[..8]),
                role: "admin".to_string(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ObservalError::Api { status, body });
        }

        let user: CreateUserResponse = resp.json().await?;
        info!(email = %user.email, "Observal user created");
        Ok((user.email, user.password))
    }

    /// Login as a specific user and get their tokens
    pub async fn login_user(
        &self,
        email: &str,
        password: &str,
    ) -> Result<ObservalTokens, ObservalError> {
        let resp = self
            .http
            .post(format!("{}/api/v1/auth/login", self.api_url))
            .json(&LoginRequest {
                email: email.to_string(),
                password: password.to_string(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ObservalError::Api { status, body });
        }

        let login: LoginResponse = resp.json().await?;
        Ok(ObservalTokens {
            access_token: login.access_token,
            refresh_token: login.refresh_token,
        })
    }

    /// Create a user and get their tokens in one call
    pub async fn provision_session_user(
        &self,
        session_id: &str,
    ) -> Result<ObservalTokens, ObservalError> {
        let (email, password) = self.create_user(session_id).await?;
        self.login_user(&email, &password).await
    }

    /// Health check — returns true if Observal API is reachable
    pub async fn is_healthy(&self) -> bool {
        let resp = self
            .http
            .get(format!("{}/readyz", self.api_url))
            .send()
            .await;

        match resp {
            Ok(r) => r.status().is_success(),
            Err(e) => {
                warn!(error = %e, "Observal health check failed");
                false
            }
        }
    }

    /// Get the API URL (for runtime-to-Observal communication)
    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// Get the container-accessible API URL (for injecting into containers)
    pub fn container_api_url(&self) -> &str {
        &self.container_api_url
    }

    /// Get the admin token (for insights elevation proxy)
    pub fn admin_token_value(&self) -> Option<&str> {
        self.admin_token.as_deref()
    }
}
