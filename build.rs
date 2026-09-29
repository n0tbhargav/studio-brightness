fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).unwrap();

    // Embed the app icon (assets/app.res, made by tools/make_icon.py) into Windows executables.
    println!("cargo:rerun-if-changed=assets/app.res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let res = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/app.res");
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}
