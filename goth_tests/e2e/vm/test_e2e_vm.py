"""End to end tests for requesting VM tasks using goth REST API client."""

import json
import logging
import os
from pathlib import Path
from typing import List

import pytest

from goth.configuration import load_yaml, Override
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import vm_exe_script, vm_task_package
from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.e2e_vm")


@pytest.mark.asyncio
async def test_e2e_vm(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting a Blender task with goth REST API client."""

    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=Path(__file__).parent / "assets",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        # Market
        demand = (
            DemandBuilder(requestor)
            .props_from_template(vm_task_package)
            .constraints("(&(golem.runtime.name=vm))")
            .build()
        )

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            providers,
            lambda proposal: proposal.properties.get("golem.runtime.name") == "vm",
        )

        # Activity
        output_file = "out0000.png"
        output_path = Path(runner.web_root_path) / "upload" / output_file
        if output_path.exists():
            os.remove(output_path)

        exe_script = vm_exe_script(runner, output_file)
        num_commands = len(exe_script)

        for agreement_id, provider in agreement_providers:
            logger.info("Running activity on %s", provider.name)
            activity_id = await requestor.create_activity(agreement_id)
            await provider.wait_for_exeunit_started()
            batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
            await requestor.collect_results(
                activity_id, batch_id, num_commands, timeout=300
            )
            await requestor.destroy_activity(activity_id)
            await provider.wait_for_exeunit_finished()

        assert output_path.is_file()
        assert output_path.stat().st_size > 0

        # Payment
        for agreement_id, provider in agreement_providers:
            await provider.wait_for_invoice_sent()
            invoices = await requestor.gather_invoices(agreement_id)
            assert all(inv.agreement_id == agreement_id for inv in invoices)
            # TODO:
            await requestor.pay_invoices(invoices)
            await provider.wait_for_invoice_paid()
