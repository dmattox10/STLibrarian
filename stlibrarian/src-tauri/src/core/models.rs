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
    /// Enclosed volume in cubic millimetres; 0 when the mesh is not closed.
    pub volume: f64,
    /// Guessed from the path when scanned, and editable. Empty means unknown.
    pub creator: String,
    pub source: String,
    /// Set only when someone groups files together; scanning never assumes it.
    pub package_id: Option<i64>,
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
    pub creator: Option<String>,
    pub package: Option<i64>,
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

/// A group of files that are one model -- body, arms, wings, supported and
/// unsupported versions.
///
/// Membership is always a decision, never an inference. Scanning proposes
/// groupings; it does not apply them, because a folder full of STLs is just as
/// often a folder full of unrelated models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub id: i64,
    pub name: String,
    pub root: String,
    pub parts: i64,
    pub total_bytes: i64,
}

/// A grouping the scanner thinks is probably one model, offered for approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSuggestion {
    pub name: String,
    pub root: String,
    pub model_ids: Vec<i64>,
    pub parts: i64,
}

/// Files that are byte-for-byte the same model stored more than once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub name: String,
    pub copies: i64,
    /// Bytes that would come back by keeping one copy.
    pub wasted_bytes: i64,
    pub models: Vec<ModelRecord>,
}
