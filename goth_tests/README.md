# goth integration tests

## Introduction

This is the base directory for all of `yagna`'s integration tests which make use of [`goth`](https://github.com/golemfactory/goth) (GOlem Test Harness).

`goth` is a Python package which is a backend and a runner for `yagna` integration tests. This directory is home to a Python project which defines a number of test cases and makes use of `goth` to execute them on an isolated Golem network.

### Test directory structure

Tests are organised into a directory structure based on their characteristics:

```
.
├── assets                                  # Contains files used by the `goth` runner directly or by test cases.
│   ├── docker
│   │   ├── yagna-goth.Dockerfile
│   │   ├── docker-compose.yml              # Compose file for the static part of each test's network
│   │   └── ...
│   ├── goth-config.yml                     # Default test network configuration
│   └── ...
├── domain                                  # Domain-specific tests
│   ├── exe_units
│   ├── market
│   ├── payments
│   │   ├── test_zero_amount_txs.py
│   │   └── ...
│   └── ya-provider
│       └── test_provider_multi_activity.py
├── e2e                                     # End-to-end tests
│   ├── vm
│   │   ├── assets                          # Blender-specific assets
│   │   │   ├── params.json
│   │   │   ├── scene.blend
│   │   │   └── ...
│   │   └── test_e2e_vm.py
│   └── wasi
│       └── test_e2e_wasi.py
```

In the above structure, for each test there is a separate `test_*.py` file with a single `test_*` function defined inside.

While file naming is just a convention, test function names **must** start with a `test_` prefix, otherwise they will not be discovered by `pytest`.

Domain-specific tests are placed in their appropriate directory under `domain`.

If a test requires custom assets (e.g. VM Blender test) they should be placed in a directory named `assets` alongside the test `.py` file itself.

### How these tests work

Every `goth` test operates on an isolated, local network of `yagna` nodes which gets created using Docker. Besides the `yagna` containers themselves this network also includes components such as `ya-sb-router` or `ganache` local blockchain.

For every test case the following steps are performed:

1. `docker-compose` is used to start the so-called "static" containers (e.g. local blockchain, these are defined in `docker-compose.yml`) and create a common Docker network for all containers participating in the test.
2. The test runner creates a number of Yagna containers (as defined in `goth-config.yml`) which are connected to the `docker-compose` network.
3. For each Yagna container started a so-called "probe" object is created and made available inside the test via the `Runner` object.
4. The integration test scenario is executed as defined in the function called by `pytest`.
5. Once the test is finished, all previously started Docker containers (both "static" and "dynamic") are removed.

### Logs from tests

All containers launched during an integration test record their logs in a pre-determined location. By default, this location is: `$TEMP_DIR/goth-tests`, where `$TEMP_DIR` is the path of the directory used for temporary files.

This path will depend either on the shell environment or the operating system on which the tests are being run (see [`tempfile.gettempdir`](https://docs.python.org/3/library/tempfile.html) for more details).

#### Log directory structure

```
.
└── goth_20210420_093848+0000
    ├── runner.log                      # debug console logs from the entire test session
    ├── test_e2e_vm                     # directory with logs from a single test
    │   ├── ethereum.log
    │   ├── provider_1.log              # debug logs from a single yagna node
    │   ├── provider_1_ya-provider.log  # debug logs from an agent running in a yagna node
    │   ├── provider_2.log
    │   ├── provider_2_ya-provider.log
    │   ├── proxy-nginx.log
    │   ├── proxy.log                   # HTTP traffic going into the yagna daemons recorded by a "sniffer" proxy
    │   ├── requestor.log
    │   ├── router.log
    │   ├── test.log                    # debug console logs from this test case only, duplicated in `runner.log`
    │   └── zksync.log
    └── test_e2e_wasi
        └── ...
```

## Running tests locally

### Building yagna binaries

Build yagna workspace and copy binaries used by tests to temp directory (can be skipped when using `release-tag` property).

```sh
cargo build --workspace --features=static-openssl \
&& rm -rf /tmp/goth-binaries && mkdir /tmp/goth-binaries \
&& cp target/x86_64-unknown-linux-musl/debug/{yagna,ya-provider,exe-unit,golemsp,gftp} /tmp/goth-binaries
```

#### Docker engine

To install Docker, follow [these instructions](https://docs.docker.com/engine/install/).
To verify your installation you can run the following command:

```sh
docker run hello-world
```

#### Python env setup

This project requires Linux environment and uses [`poetry`](https://python-poetry.org/) to manage its dependencies.

Python setup

```sh
# Tests use Python 3.8+
# It can be installed using `virtualenv`
# Install it if missing (or use a different Python env manager, or use OS native Python)
sudo apt-get install virtualenv

# Initialize virtualenv
virtualenv .venv --python=python3.8

# Switch to installed Python 3.8 environment
source .venv/bin/activate
```

Install goth dependencies

```sh
cd goth_tests
# Install poetry
pip install poetry
# Install dependencies
poetry install
```

#### GitHub API token

`goth` makes use of the GitHub API to download releases and artifacts for its test runs. Although all of these assets are public, using the GitHub API still requires basic authentication. Therefore you need to provide `goth` with a personal access token.

To generate a new token, go to your account's [developer settings](https://github.com/settings/tokens).
You will need to grant your new token the `public_repo` scope, as well as the `read:packages` scope. The packages scope is required in order to pull Docker images from GitHub.

Once your token is generated you need to do two things:

1. Log in to GitHub's Docker registry by calling: `docker login docker.pkg.github.com -u {username}`, replacing `{username}` with your GitHub username and pasting in your access token as the password. You only need to do this once on your development machine.
2. Export an environment variable named `GITHUB_API_TOKEN` and use the access token as its value. This environment variable will need to be available in the shell from which you run the integration tests.

#### Running a test session

All of the commands below assume you are running from the tests' root directory (`goth_tests`) and that you have a GitHub token available in your shell.

You can run the whole test suite by calling:

```sh
poetry run poe goth-tests --config-override docker-compose.build-environment.binary-path=/tmp/goth-binaries
```

To run a single test you can call `pytest` directly like so:

```sh
poetry run pytest --config-override docker-compose.build-environment.binary-path=/tmp/goth-binaries -svx e2e/vm/test_e2e_vm.py
```

#### Running a test session using specific release of `yagna`

```sh
poetry run poe goth-tests --config-override docker-compose.build-environment.release-tag=v0.13.0
```

## Writing test cases

```py
from pathlib import Path
from typing import List, Optional

from goth.configuration import load_yaml, Override
from goth.runner.probe import RequestorProbe
from goth_tests.helpers.probe import ProviderProbe

@pytest.mark.asyncio
async def test_example(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    goth_config = load_yaml(common_assets / "goth-config.yml", config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    async with runner(goth_config.containers):
        providers = runner.get_probes(probe_type=ProviderProbe)
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]

        drivers = requestor.cli.payment_drivers()
        assert drivers and drivers.items()

        for provider in providers:
            await provider.wait_for_offer_subscribed()
```

The above is an example of a test case, showcasing some of the basic `goth` functionalities.

```py
@pytest.mark.asyncio
async def test_e2e_wasi(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
```

Functions which names' start with `test` are considered test cases by `pytest`. As such, they can receive **fixture arguments** using the names of the fixtures. These are provided by functions decorated with `@pytest.fixture`, common fixtures used in this project can be found in `goth_tests/conftest.py`.

For more information on fixtures see [relevant `pytest` docs](https://docs.pytest.org/en/stable/fixture.html).

```py
goth_config = load_yaml(common_assets / "goth-config.yml", config_overrides)
```

Configuration files can be specified on a per-test basis. The argument `config_overrides` allows for overriding or adding fields to the config. For more information, see the `Test configuration` section below.

```py
runner = Runner(
    base_log_dir=log_dir,
    compose_config=goth_config.compose_config,
    web_root_path=common_assets / "web-root",
)

async with runner(goth_config.containers):
    ...
```

The `Runner` class is the main entrypoint of `goth`. Its async context manager (`async with runner()`) handles setting up and tearing down the test Golem network.

Inside the runner's context you can obtain `Probe` objects:

```py
providers = runner.get_probes(probe_type=ProviderProbe)
requestor = runner.get_probes(probe_type=RequestorProbe)[0]
```

`Probe` is `goth`'s interface for interacting with `yagna` nodes running as part of a test. These objects contain modules for interacting with the node's REST API, executing CLI commands and interacting with the Docker API.

Finally, the sample test includes some simple examples of how these modules can be used:

```py
drivers = requestor.cli.payment_drivers()
assert drivers and drivers.items()

for provider in providers:
    await provider.wait_for_offer_subscribed()
```

There's much more to `goth` that's not covered in this overview. Some of the more advanced features are: event assertions, event monitors, running host commands etc.

Make sure to take a look at the integration tests in this repository, as well as the [ones in `yapapi`](https://github.com/golemfactory/yapapi/tree/master/tests/goth).

### Test configuration

#### `goth-config.yml`

`goth` can be configured using a YAML file. The default `goth-config.yml` is located in the common, generated assets (i.e. `goth_tests/assets`) and looks something like this:

```yml
docker-compose:

  docker-dir: "docker"                          # Where to look for docker-compose.yml and Dockerfiles

  build-environment:                            # Fields related to building the yagna Docker image
    # binary-path: ...
    # deb-path: ...
    # branch: ...
    # commit-hash: ...
    # release-tag: ...

  compose-log-patterns:                         # Log message patterns used for container ready checks
    ethereum-mainnet: ".*Wallets supplied."
    ethereum-goerli: ".*Wallets supplied."
    ethereum-polygon: ".*Wallets supplied."
    ...

key-dir: "keys"                                 # Where to look for pre-funded Ethereum keys

node-types:                                     # User-defined node types to be used in `nodes`
  - name: "Requestor"
    class: "goth.runner.probe.RequestorProbe"

  - name: "Provider"
    class: "goth_tests.helpers.probe.ProviderProbe"
    mount: ...

nodes:                                          # List of yagna nodes to be run in the test
  - name: "requestor"
    type: "Requestor"

  - name: "provider-1"
    type: "Provider"
    use-proxy: True
```

#### Configuration overrides

It's also possible to override parts of the configuration. This is useful for whenever you want to change some config value without having to edit the `goth-config.yml` file.

When running tests you can use the `--config-override` parameter:

```sh
poetry run poe goth-tests --config-override docker-compose.build-environment.commit-hash=29b7f85
```

Overrides specified as CLI arguments are passed into the test functions through the `config_overrides` fixture:

```sh
async def test_something(
    config_overrides: List[Override],     # This is provided by a fixture
    ...
):
    goth_config = load_yaml(Path("path/to/goth-config.yml", config_overrides)
```

### Maintenance

#### Updating expected `yagna` log lines

Some `goth` test steps rely on scanning the logs from a process running on one or more `yagna` nodes. In particular, this is used extensively in the case of provider agent logs (e.g. determining if the exeunit has finished successfully).

Since log messages are subject to change, these log line patterns need to be kept up to date.

`yagna` tests implement a custom `ProviderProbe` class (`goth_tests/helpers/probe.py`) which includes all the commonly used log patterns as its methods.

When a test fails due to an updated log message, the expected pattern can be updated by modifying one of `ProviderProbe`'s methods, e.g.:

```py
@step()
async def wait_for_exeunit_finished(self):
    """Wait until exe-unit finishes."""
    await self.provider_agent.wait_for_log(
        r"(.*)ExeUnit process exited with status Finished - exit code: 0(.*)"
    )
```
