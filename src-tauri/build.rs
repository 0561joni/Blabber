fn main() {
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=11.0");

    tauri_build::build()
}
