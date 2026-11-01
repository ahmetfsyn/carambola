fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/notification/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("carambola_notification");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=UserNotifications");
        println!("cargo:rerun-if-changed=src/notification/macos.m");
    }
    tauri_build::build()
}
