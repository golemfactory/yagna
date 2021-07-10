import pytest
from datetime import datetime, timezone
from goth.configuration import Override
from goth.runner.log import configure_logging
from pathlib import Path
from typing import List


def pytest_addoption(parser):
    parser.addoption(
        "--config-override",
        action="append",
        help="Set an override for a value specified in goth-config.yml file. \
                This argument may be used multiple times. \
                Values must follow the convention: {yaml_path}={value}, e.g.: \
                `docker-compose.docker-dir=/tmp/some_dir",
    )


@pytest.fixture(scope="session")
def common_assets() -> Path:
    """Fixture providing path to dir containing generated goth assets."""
    assets_path = Path(__file__).parent / "assets"
    return assets_path.resolve()


@pytest.fixture(scope="session")
def default_config() -> Path:
    """Fixture providing path to yagna's default goth config."""
    config_path = Path(__file__).parent / "goth-config.yml"
    return config_path.resolve()


@pytest.fixture(scope="session")
def log_dir() -> Path:
    """Fixture providing unique directory for logs from a test run."""
    base_dir = Path("/", "tmp", "goth-tests")
    date_str = datetime.now(tz=timezone.utc).strftime("%Y%m%d_%H%M%S%z")
    log_dir = base_dir / f"goth_{date_str}"
    log_dir.mkdir(parents=True)

    configure_logging(log_dir)

    return log_dir


@pytest.fixture(scope="function")
def config_overrides(request) -> List[Override]:
    """Fixture parsing --config-override params passed to the test invocation."""
    overrides: List[str] = request.config.option.config_override or []
    return [tuple(o.split("=")) for o in overrides]
