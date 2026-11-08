fn main() {
    slint_build::compile("ui/overlay.slint").expect("compile transparent overlay prototype");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/carambola.rc", embed_resource::NONE)
            .manifest_required()
            .expect("embed Carambola Windows icon and version information");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/platform/macos_vision.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("carambola_vision");
        for framework in ["Vision", "Foundation", "CoreGraphics"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rerun-if-changed=src/platform/macos_vision.m");
    }
}
