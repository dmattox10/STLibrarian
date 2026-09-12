fn main() {
    // tauri_build::build() bakes in the Tauri context and expects the desktop
    // toolchain. The server build has neither and must not call it.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        tauri_build::build();
    }

    // rust-embed resolves its folder at compile time and refuses to compile if
    // it is not there. The server can legitimately be built before `yarn
    // build` has ever run -- CI builds the two halves in parallel, and a fresh
    // clone has no dist/ at all -- so make sure the directory exists. An empty
    // one embeds an empty asset set and the server says so at startup, which
    // is a far better failure than a build that cannot start.
    if std::env::var("CARGO_FEATURE_SERVER").is_ok() {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let dist = std::path::Path::new(&manifest).join("..").join("dist");
        std::fs::create_dir_all(&dist).ok();
        // Rebuild when the front end is rebuilt, or a `yarn build` would not
        // reach a release binary until something else happened to touch Rust.
        println!("cargo:rerun-if-changed={}", dist.display());
    }
}
