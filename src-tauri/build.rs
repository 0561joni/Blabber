fn main() {
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=11.0");

    build_qwen_asr();

    tauri_build::build()
}

fn build_qwen_asr() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        return;
    }

    let vendor = "vendor/qwen-asr";
    for header in [
        "qwen_asr.h",
        "qwen_asr_audio.h",
        "qwen_asr_kernels.h",
        "qwen_asr_kernels_impl.h",
        "qwen_asr_safetensors.h",
        "qwen_asr_tokenizer.h",
    ] {
        println!("cargo:rerun-if-changed={vendor}/{header}");
    }
    let sources = [
        "qwen_asr.c",
        "qwen_asr_kernels.c",
        "qwen_asr_kernels_generic.c",
        "qwen_asr_kernels_neon.c",
        "qwen_asr_kernels_avx.c",
        "qwen_asr_audio.c",
        "qwen_asr_encoder.c",
        "qwen_asr_decoder.c",
        "qwen_asr_tokenizer.c",
        "qwen_asr_safetensors.c",
    ];

    let mut build = cc::Build::new();
    build
        .include(vendor)
        .define("USE_BLAS", None)
        .opt_level(3)
        .flag_if_supported("-ffast-math")
        .flag_if_supported("-Wno-unused-parameter");
    for source in sources {
        let path = format!("{vendor}/{source}");
        println!("cargo:rerun-if-changed={path}");
        build.file(path);
    }

    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            println!("cargo:rustc-link-lib=framework=Accelerate");
        }
        Ok("linux") => {
            build.define("USE_OPENBLAS", None);
        }
        _ => {}
    }

    build.compile("qwen_asr");
    // Keep this explicit in addition to cc's emitted metadata. Cargo may reuse the
    // host build script after a cross-target build where Qwen is intentionally
    // skipped, and the native archive must still be linked for the host target.
    println!("cargo:rustc-link-lib=static=qwen_asr");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=pthread");
}
