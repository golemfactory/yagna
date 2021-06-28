"""End to end tests for requesting WASM tasks using goth REST API clients."""

import json
import logging
from pathlib import Path
from typing import List

import pytest

from goth.address import (
    PROXY_HOST,
    YAGNA_REST_URL,
)
from goth.configuration import load_yaml, Override
from goth.node import node_environment
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.e2e_wasi")


@pytest.mark.asyncio
async def test_e2e_wasi(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting WASM tasks with goth REST API client."""

    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        # Market
        demand = DemandBuilder(requestor).props_from_template(wasi_task_package).build()

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            providers,
            lambda p: p.properties.get("golem.runtime.name") == "wasmtime",
        )

        # Activity
        exe_script = wasi_exe_script(runner)
        num_commands = len(exe_script)

        for agreement_id, provider in agreement_providers:
            logger.info("Running activity on %s", provider.name)
            activity_id = await requestor.create_activity(agreement_id)
            await provider.wait_for_exeunit_started()
            batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
            await requestor.collect_results(
                activity_id, batch_id, num_commands, timeout=30
            )
            await requestor.destroy_activity(activity_id)
            await provider.wait_for_exeunit_finished()

        # Payment
        for agreement_id, provider in agreement_providers:
            await provider.wait_for_invoice_sent()
            invoices = await requestor.gather_invoices(agreement_id)
            assert all(inv.agreement_id == agreement_id for inv in invoices)
            await requestor.pay_invoices(invoices)
            await provider.wait_for_invoice_paid()
