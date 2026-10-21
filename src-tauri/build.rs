fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Embed the shared manifest through the linker for every final target.
        // Avoid a second resource with the same ID from Tauri's default manifest.
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("Tauri build configuration");
        let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-common-controls.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        // The generic link argument also reaches the library's unit-test binary;
        // rustc-link-arg-tests covers standalone integration targets only.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    } else {
        tauri_build::build();
    }
}
