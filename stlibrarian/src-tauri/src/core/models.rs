// ================================
// core/models.rs
// ================================
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelRecord {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub hash: String,
    pub tags: Vec<String>,
}
