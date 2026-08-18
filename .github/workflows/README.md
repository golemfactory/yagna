# CI layout

Pull requests and regular branch pushes enter through `ci.yml`.

The first stage is a single Linux gate. It checks the lockfile and workflow
syntax, formatting, workspace Clippy, and the standard workspace unit tests.
All of these commands run in one job so they can reuse the same Rust build
directory. The gate disables Rust debug information and incremental state;
these artifacts are not needed on an ephemeral runner and otherwise exceed the
14 GB storage available on standard GitHub-hosted runners. The dependency cache
does not upload the complete `target` directory, which keeps repository cache
usage bounded and avoids restoring large build trees from unrelated revisions.

The more expensive jobs start only after the Linux gate succeeds:

- macOS unit tests;
- Linux and macOS market tests;
- Linux system tests;
- AArch64 builds.

Windows and E2E checks are opt-in on pull requests. Add `ci:windows` to run
Windows unit tests, market tests, and the Windows cross-build. Add `ci:e2e` to
build integration binaries on `yagna-builder` and run Goth and payment tests.
Adding either label cancels any older run for the same pull request and restarts
the pipeline. The selected optional jobs start after the new Linux gate passes.
Trusted branch pushes and manual workflow runs still execute the complete
matrix.

`mise.toml` is the single source of truth for tool versions and
developer-facing CI commands. CI installs those tools through the repository's
`setup-tools` composite action. Cargo-based tools use the pinned
`cargo-binstall` binary when a prebuilt release is available. The action also
sets `PROTOC` explicitly, preventing Rust build scripts from downloading their
own compiler.
Run the complete first-stage check locally with:

```shell
mise install actionlint rust protoc shellcheck
export PROTOC="$(mise which protoc)"
mise run ci:gate
```

Pushes to the same pull request cancel older CI runs. Branch builds and pull
requests from this repository update the Cargo dependency cache; fork pull
requests are read-only.
