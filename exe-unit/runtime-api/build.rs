use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_SHOW_GENPATH");
    if env::var("BUILD_SHOW_GENPATH").is_ok() {
        println!(
            "cargo:warning=Generating code into {}",
            env::var("OUT_DIR").unwrap()
        );
    }

    let mut config = prost_build::Config::default();

    config.type_attribute(
        "Endpoint",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );

    config
        .compile_protos(&["src/runtime.proto"], &["src/"])
        .unwrap();
}
