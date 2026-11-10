use std::{env, fs, path::PathBuf};

fn main() {
    slint_build::compile("ui/overlay.slint").expect("compile transparent overlay prototype");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
        let version = env::var("CARGO_PKG_VERSION").expect("Cargo sets CARGO_PKG_VERSION");
        let parts = version
            .split('.')
            .map(|part| {
                part.parse::<u16>()
                    .expect("Windows version must be numeric")
            })
            .collect::<Vec<_>>();
        assert_eq!(parts.len(), 3, "Windows version must be major.minor.patch");
        assert_eq!(
            parts[2], 0,
            "Cargo patch version must be zero; public Carambola releases use MAJOR.MINOR"
        );
        let public_version = format!("{}.{}", parts[0], parts[1]);

        let icon = PathBuf::from(
            env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("assets/carambola.ico");
        let icon = icon.to_string_lossy().replace('\\', "\\\\");
        let version_fields = format!("{},{},{},0", parts[0], parts[1], parts[2]);
        let rc_template =
            fs::read_to_string("assets/carambola.rc").expect("read Windows resource template");
        let rc = rc_template
            .replace("@ICON_PATH@", &icon)
            .replace("@VERSION_FIELDS@", &version_fields)
            .replace("@PUBLIC_VERSION@", &public_version);
        assert!(
            !rc.contains('@'),
            "Windows resource template contains an unresolved placeholder"
        );
        let rc_path = out_dir.join("carambola.rc");
        fs::write(&rc_path, rc).expect("write generated Windows version resource");
        println!("cargo:rerun-if-changed=assets/carambola.ico");
        println!("cargo:rerun-if-changed=assets/carambola.rc");

        embed_resource::compile(rc_path, embed_resource::NONE)
            .manifest_required()
            .expect("embed Carambola Windows icon and version information");
    }

    println!("cargo:rerun-if-changed=build.rs");

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
