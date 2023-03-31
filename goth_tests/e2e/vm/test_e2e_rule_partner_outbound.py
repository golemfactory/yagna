"""End to end tests for requesting VM tasks using goth REST API client."""

import json
import logging
import os
import base64
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

from goth_tests.helpers.activity import vm_exe_script_outbound
from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.rule_partner_outbound")


@pytest.mark.asyncio
async def test_e2e_rule_partner_outbound(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting a task using outbound network feature. Partner rule negotiation scenario."""

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
        provider = runner.get_probes(probe_type=ProviderProbe)[0]

        manifest = open(f"{runner.web_root_path}/outbound_manifest.json").read()
        node_descriptor = open(f"{runner.web_root_path}/test_e2e_rule_partner_outbound/node-descriptor.signed.json").read()

        # Market
        demand = (
            DemandBuilder(requestor)
            .props_from_template(task_package = None)
            .property("golem.srv.comp.payload", base64.b64encode(manifest.encode()).decode())
            .property("golem.node.descriptor", node_descriptor)
            .constraints("(&(golem.runtime.name=vm))")
            .build()
        )

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            [provider],
            lambda proposal: proposal.properties.get("golem.runtime.name") == "vm",
        )

        agreement_id, provider = agreement_providers[0]

        # Activity

        output_file = "output.txt"
        output_path = Path(runner.web_root_path) / "upload" / output_file

        exe_script = vm_exe_script_outbound(runner, output_file)

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

        assert output_path.is_file()
        assert len(output_path.read_text()) > 0

        # Payment

        await provider.wait_for_invoice_sent()
        invoices = await requestor.gather_invoices(agreement_id)
        assert all(inv.agreement_id == agreement_id for inv in invoices)
        await requestor.pay_invoices(invoices)
        await provider.wait_for_invoice_paid()
