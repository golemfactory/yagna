# Testing framework based tests

Test framework should be use, when test requires running yagna daemon in the background.
Developer can use Cli, GSB and REST calls to test his code.

## Running

All tests are hidden behind `framework-test` feature flag and are disabled by default.

To run all tests including framework tests and unit tests (but without market test suite), run:
`cargo test --workspace --features framework-test`

To run only framework tests use command:
`cargo test --test '*' -p yagna -p ya-exe-unit -p ya-transfer --features framework-test`

## Creating tests

```
#[cfg_attr(not(feature = "framework-test"), ignore)]
#[framework_test]
async fn test_appkey_removal(framework: YagnaFramework) -> anyhow::Result<()> {
    let yagna = framework
        // Create new yagna node named `node1`.
        .new_node("node1")
        // Spawn yagna instance.
        .service_run().await?;
    
    // Run CLI command on `node1` yagna instance.
    yagna
        .command()
        .arg("app-key")
        .arg("create")
        .arg("test-appkey")
        .assert()
        .success();
}
```

## Test directories

Yagna requires data directory provided to work correctly.
Each test is placed in separate directory under `$CARGO_TARGET_TMPDIR/{test name}`.
Each yagna node has its data directory in `$CARGO_TARGET_TMPDIR/{test name}/{node name}`.

Directories are cleared on test start, but they remain after execution.
