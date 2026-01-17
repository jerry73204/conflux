use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    // Generate header file
    let config_path = format!("{}/cbindgen.toml", crate_dir);

    // Generate to OUT_DIR
    let bindings = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(
            cbindgen::Config::from_file(&config_path).expect("Unable to find cbindgen.toml"),
        )
        .generate()
        .expect("Unable to generate bindings");

    bindings.write_to_file(PathBuf::from(&out_dir).join("conflux_ffi.h"));

    // Also write to a known location for the C++ build
    let header_path = PathBuf::from(&crate_dir)
        .parent()
        .unwrap()
        .join("include")
        .join("conflux_ffi.h");

    if let Some(parent) = header_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let bindings2 = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(
            cbindgen::Config::from_file(&config_path).expect("Unable to find cbindgen.toml"),
        )
        .generate()
        .expect("Unable to generate bindings");

    bindings2.write_to_file(&header_path);

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
}
