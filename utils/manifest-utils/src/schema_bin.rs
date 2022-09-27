extern crate ya_manifest_utils;

use schemars::schema_for;

use ya_manifest_utils::manifest::AppManifest;

fn main() {
    let schema = schema_for!(AppManifest);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
