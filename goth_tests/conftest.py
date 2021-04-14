from datetime import datetime, timezone
from pathlib import Path

import pytest

from goth.runner.log import configure_logging


@pytest.fixture(scope="session")
def common_assets() -> Path:
    assets_path = Path(__file__).parent / "assets"
    return assets_path.resolve()


@pytest.fixture(scope="session")
def log_dir() -> Path:
    base_dir = Path("/", "tmp", "goth-tests")
    date_str = datetime.now(tz=timezone.utc).strftime("%Y%m%d_%H%M%S%z")
    log_dir = base_dir / f"goth_{date_str}"
    log_dir.mkdir(parents=True)

    configure_logging(log_dir)

    return log_dir
