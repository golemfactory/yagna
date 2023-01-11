use actix_rt;
use anyhow;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::YagnaMock;

pub use macros::prepare_test_dir;
pub use ya_framework_macro::framework_test;

#[derive(Clone)]
pub struct YagnaFramework {
    inner: Arc<Mutex<YagnaNetworkImpl>>,
    test_dir: PathBuf,
    test_name: String,
}

/// Entities that require tear down after test is finished.
/// This allows us to kill yagna process even in case of panic.
#[derive(Clone)]
pub struct YagnaNetworkImpl {
    nodes: Vec<YagnaMock>,
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
    pub use serial_test;
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

/// https://lik.ai/blog/async-setup-and-teardown-in-rust
pub fn framework_setup<T, F>(test_fn: T, test_dir: &Path, test_name: &str)
where
    T: FnOnce(YagnaFramework) -> F + std::panic::UnwindSafe,
    F: Future<Output = anyhow::Result<()>>,
{
    let framework = YagnaFramework::new(test_dir, test_name);
    let framework_ = framework.clone();

    let result = std::panic::catch_unwind(|| {
        actix_rt::System::new().block_on(async move { test_fn(framework).await })
    });

    if let Err(e) = actix_rt::System::new().block_on(async { framework_.tear_down().await }) {
        println!("Error during Yagna framework tear down: {e}");
    };

    result.unwrap().unwrap();
}

impl YagnaFramework {
    pub fn new(test_dir: impl Into<PathBuf>, test_name: impl Into<String>) -> YagnaFramework {
        YagnaFramework {
            inner: Arc::new(Mutex::new(YagnaNetworkImpl { nodes: vec![] })),
            test_dir: test_dir.into(),
            test_name: test_name.into(),
        }
    }

    pub fn new_node(&self, name: impl ToString) -> YagnaMock {
        let yagna_dir = self.test_dir.join(&self.test_name).join(name.to_string());
        let yagna = YagnaMock::new(&yagna_dir);
        {
            self.inner.lock().unwrap().nodes.push(yagna.clone());
        }
        yagna
    }

    pub(crate) async fn tear_down(&self) -> anyhow::Result<()> {
        let timeout = std::time::Duration::from_secs(5);
        let nodes = {
            self.inner
                .lock()
                .map_err(|e| anyhow::anyhow!("Tear down - failed to acquire mutex. Error: {e}"))?
                .nodes
                .clone()
        };
        let mut futures = nodes
            .into_iter()
            .fold(tokio::task::JoinSet::new(), |mut set, yagna| {
                set.spawn_local(async move { yagna.tear_down(timeout).await });
                set
            });

        for result in futures.join_next().await {
            match result {
                Err(e) => log::error!("Error waiting for yagna shutdown: {e}"),
                Ok(Err(e)) => log::error!("Error waiting for yagna shutdown: {e}"),
                _ => continue,
            }
        }

        Ok(())
    }
}
