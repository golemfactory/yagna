"""Tests payment driver list CLI command."""

import logging
from pathlib import Path
from typing import List
from time import sleep
from datetime import datetime, timedelta, timezone

import pytest
from ya_payment.exceptions import ApiException

from goth.address import (
    PROXY_HOST,
    YAGNA_REST_URL,
)
from goth.configuration import load_yaml, Override
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

logger = logging.getLogger("goth.test.payments.release-allocations")


@pytest.mark.asyncio
async def test_payment_release_allocations(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test just the requestor's CLI command and automatic allocation timeout logic, no need to set up provider."""

    nodes = [
        {"name": "requestor", "type": "Requestor"},
    ]
    config_overrides.append(("nodes", nodes))
    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        deadline = 3
        timeout = datetime.now(timezone.utc) + timedelta(seconds=deadline)

        # Test requestor's CLI command for releasing allocations

        allocation = await requestor.create_allocation()

        # Confirming that allocation exists
        assert await requestor.get_allocation(allocation)

        requestor.cli.payment_release_allocations()

        # Confirming that allocation does not exist after release
        with pytest.raises(ApiException):
            await requestor.get_allocation(allocation)

        # Test allocation timeout logic

        allocation = await requestor.create_allocation(timeout)

        # Confirming that allocation exists
        assert await requestor.get_allocation(allocation)

        # Wait some time to ensure that specified timeout has passed
        waiting_time = deadline * 2
        sleep(waiting_time)

        # Confirming that allocation does not exist after release
        with pytest.raises(ApiException):
            await requestor.get_allocation(allocation)
