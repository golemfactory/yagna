extern crate vergen;
use vergen::{generate_cargo_keys, ConstantsFlags};

fn main() {
    let flags =
        ConstantsFlags::SHA_SHORT | ConstantsFlags::BUILD_DATE | ConstantsFlags::SEMVER_LIGHTWEIGHT;
    generate_cargo_keys(flags).expect("Unable to generate the cargo keys");
}
