"""Tests payment validate allocations"""

import logging
from pathlib import Path
from typing import List
from datetime import datetime, timezone

import pytest
from ya_payment.exceptions import ApiException
from ya_payment.models import Allocation

from goth.address import (
    PROXY_HOST,
    YAGNA_REST_URL,
)
from goth.configuration import load_yaml, Override
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

logger = logging.getLogger("goth.test.payments.validate-allocations")


@pytest.mark.asyncio
async def test_payment_validate_allocations(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test just the requestor's CLI command and validate allocations, no need to set up provider."""

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

        # Test requestor's CLI command for allocation validation

        # Make initial payment fund 1000GLM
        requestor.cli.payment_fund(payment_driver="erc20")

        # Allocation bigger than total amount should not be possible
        with pytest.raises(ApiException):
            await requestor.create_allocation(total_amount=2000)


        # Allocate 600 GLM
        allocation = await requestor.create_allocation(total_amount=600)

        # Confirming that allocation exists
        assert await requestor.get_allocation(allocation.allocation_id)

        # Allocation bigger than reserved amount should not be possible
        with pytest.raises(ApiException):
            await requestor.create_allocation(total_amount=600)
