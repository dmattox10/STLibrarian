// ================================
// core/auth.rs (VERY BASIC)
// ================================
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
}

pub fn login(username: &str, password: &str) -> Option<AuthResponse> {
    if username == "admin" && password == "admin" {
        Some(AuthResponse {
            token: "dev-token".into(),
        })
    } else {
        None
    }
}
