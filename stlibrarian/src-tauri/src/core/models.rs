use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecord {
    pub id: i64,
    pub path: String,
    /// File stem, kept denormalised so the grid can sort and search without
    /// splitting paths on every render.
    pub name: String,
    pub size: i64,
    pub hash: String,
    pub triangles: i64,
    /// Bounding box in the STL's own units (millimetres, by near-universal
    /// convention in this hobby).
    pub dim_x: f64,
    pub dim_y: f64,
    pub dim_z: f64,
    pub has_thumb: bool,
    pub mtime: i64,
    pub tags: Vec<String>,
    /// Names of the collections holding this model. Denormalised onto the row
    /// so the inspector can show membership without a second round trip.
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// Everything the library grid can ask for. All fields optional so a bare
/// `{}` means "the whole library, newest first".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryQuery {
    pub search: Option<String>,
    /// A model must carry *every* tag listed here, not any of them. Narrowing
    /// is what a filter is for.
    pub tags: Vec<String>,
    /// Restrict to one collection. Combines with search and tags rather than
    /// replacing them, so you can search inside a collection.
    pub collection: Option<i64>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryStats {
    pub models: i64,
    pub total_bytes: i64,
    pub tags: i64,
    pub missing: i64,
    pub without_thumb: i64,
}

/// A curated, named set of models.
///
/// Distinct from a tag on purpose: a tag describes a property every member
/// happens to share, a collection is simply the set of things you put in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

/// How many rows a bulk operation actually changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BulkResult {
    pub changed: usize,
}
