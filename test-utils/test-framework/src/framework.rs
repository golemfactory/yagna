use std::path::PathBuf;

use crate::YagnaMock;

pub use macros::prepare_test_dir;

pub struct YagnaNetwork {
    nodes: Vec<YagnaMock>,
    test_dir: PathBuf,
    test_name: String,
}

pub mod macros {
    #[macro_export]
    macro_rules! prepare_test_dir {
        () => {
            // CARGO_TARGET_TMPDIR is available in compile time only in binary modules, so we can't
            // use it in this library. Thanks to using macro it will be resolved in final code not here
            // and it will work.
            ya_test_framework::framework::prepare_test_dir_(env!("CARGO_TARGET_TMPDIR"))
        };
    }
    pub use prepare_test_dir;
}

pub fn prepare_test_dir_(dir: &str) -> PathBuf {
    let test_dir = PathBuf::from(&dir);

    #[allow(unused_must_use)]
    {
        std::fs::remove_dir_all(&test_dir); // ignores error if does not exist
        std::fs::create_dir_all(&test_dir);
    }

    test_dir
}
