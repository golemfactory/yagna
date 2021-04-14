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
from goth.configuration import load_yaml
from goth.node import node_environment
from goth.runner import Runner
from goth.runner.container.payment import PaymentIdPool
from goth.runner.container.yagna import YagnaContainerConfig
from goth.runner.probe import ProviderProbe, RequestorProbe

from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.activity import vm_exe_script

logger = logging.getLogger("goth.test.e2e_vm")


@pytest.mark.asyncio
async def test_e2e_vm_success(
    common_assets: Path,
    log_dir: Path,
):
    """Test successful flow requesting a Blender task with goth REST API client."""

    goth_config = load_yaml(common_assets / "goth-config.yml")

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=Path(__file__).parent / "assets",
    )

    async with runner(goth_config.containers):
        task_package = (
            "hash:sha3:9a3b5d67b0b27746283cb5f287c13eab1beaa12d92a9f536b747c7ae:"
            "http://3.249.139.167:8000/local-image-c76719083b.gvmi"
        )

        output_file = "out0000.png"

        output_path = Path(runner.web_root_path) / "upload" / output_file
        if output_path.exists():
            os.remove(output_path)

        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        demand = DemandBuilder(requestor).props_from_template(task_package).build()

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            providers,
            lambda proposal: proposal.properties.get("golem.runtime.name") == "vm",
        )

        #  Activity
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
