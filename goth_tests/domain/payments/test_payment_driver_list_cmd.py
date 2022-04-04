"""Tests payment driver list CLI command."""

import logging
from pathlib import Path
from typing import List

import pytest

from goth.configuration import load_yaml, Override
from goth.runner import Runner
from goth.runner.probe import RequestorProbe


logger = logging.getLogger("goth.test.payments.driver-list")


@pytest.mark.asyncio
async def test_payment_driver_list(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test just the requestor's CLI command, no need to setup provider."""

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
        res = requestor.cli.payment_drivers()
        assert res and res.items()
        driver = next(iter(res.values()), None)

        assert driver
        assert driver.default_network, "Default network should be set"

        network = driver.networks.get(driver.default_network, None)
        assert network, "Network should belong to the Driver"
        assert network.default_token, "Default taken should be set"

        token = network.tokens.get(network.default_token, None)
        assert token, "Token should belong to the Network"
