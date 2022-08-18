"""End to end tests for requesting VM tasks using goth REST API client."""

import json
import logging
import os
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
from goth.runner.container.payment import PaymentIdPool
from goth.runner.container.yagna import YagnaContainerConfig
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import vm_exe_script, vm_task_package
from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.e2e_outbound")


@pytest.mark.asyncio
async def test_e2e_outbound(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting a Blender task with goth REST API client."""

    # Test external api request just one Requestor and one Provider
    nodes = [
        {"name": "requestor", "type": "Requestor"},
        {"name": "provider-1", "type": "VM-Wasm-Provider", "use-proxy": True},
    ]
    config_overrides.append(("nodes", nodes))

    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=Path(__file__).parent / "assets",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        provider = runner.get_probes(probe_type=ProviderProbe)[0] #TODO probably change to one provider


        # Setup trusted certs in provider
        #TODO

        # Market
        #TODO manifest.json
        demand = (
            DemandBuilder(requestor)
            .props_from_template(vm_task_package)
            .constraints("(&(golem.runtime.name=vm))")
            .build()
        )

        agreement_id, provider = await negotiate_agreements(
            requestor,
            demand,
            [provider], #TODO is it valid list?
            lambda proposal: proposal.properties.get("golem.runtime.name") == "vm",
        )[0]

        # Activity

        #TODO make outbound script
        exe_script = vm_exe_script(runner, output_file)
        num_commands = len(exe_script)

        logger.info("Running activity on %s", provider.name)
        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()
        batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
        await requestor.collect_results(
            activity_id, batch_id, num_commands, timeout=300
        )
        await requestor.destroy_activity(activity_id)
        await provider.wait_for_exeunit_finished()

        # assert output_path.is_file()
        # assert output_path.stat().st_size > 0

        # Payment
        # Todo probably unnecessary
        for agreement_id, provider in agreement_providers:
            await provider.wait_for_invoice_sent()
            invoices = await requestor.gather_invoices(agreement_id)
            assert all(inv.agreement_id == agreement_id for inv in invoices)
            # TODO:
            await requestor.pay_invoices(invoices)
            await provider.wait_for_invoice_paid()
