fn main() {
    // tauri_build::build() bakes in the Tauri context and expects the desktop
    // toolchain. The server build has neither and must not call it.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        tauri_build::build();
    }
}
