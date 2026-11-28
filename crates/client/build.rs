fn main() {
    #[cfg(feature = "desktop-ui")]
    tauri_build::build();
}
