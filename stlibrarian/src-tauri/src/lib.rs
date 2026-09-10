pub mod core;

// The server build must not pull in Tauri; see the feature notes in Cargo.toml.
#[cfg(feature = "desktop")]
mod desktop;

#[cfg(feature = "desktop")]
pub use desktop::run;
