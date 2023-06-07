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

logger = logging.getLogger("goth.test.outbound_perf")

def vm_exe_script(runner: Runner, addr: str, output_file: str):
    """VM exe script builder."""
    """Create a VM exe script for running a outbound task."""

    output_path = Path(runner.web_root_path) / output_file
    if output_path.exists():
        os.remove(output_path)

    web_server_addr = f"http://{runner.host_address}:{runner.web_server_port}"

    return [
        {"deploy": {}},
        {"start": {}},
        {"run": {"entry_point": "/golem/entrypoints/entrypoint.sh", "args": [addr, '22235', '22236', '22237', '0.5', '10', '0']}},
        {
            "transfer": {
                "from": f"container:/golem/output/output.json",
                "to": f"{web_server_addr}/upload/{output_file}",
            }
        },
    ]

@pytest.mark.asyncio
async def test_e2e_outbound_perf(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting a task using outbound network feature. X.509 cert negotiation scenario."""

    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=Path(__file__).parent / "assets",
    )

    async with runner(goth_config.containers):
        server_addr = None
        for info in runner.get_container_info().values():
            if 'outbound-test' in info.aliases:
                server_addr = info.address
                break
        assert(server_addr is not None, "Can't find container `outbound-test`")
        logger.info("outbound-test container found at %s", server_addr)

        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        provider = runner.get_probes(probe_type=ProviderProbe)[0]

        manifest = open(f"{runner.web_root_path}/test_e2e_outbound_perf/image/manifest.json").read()
        signature = open(f"{runner.web_root_path}/test_e2e_outbound_perf/outbound_signature.sha256.base64").read()
        certificate = open(f"{runner.web_root_path}/test_e2e_outbound_perf/outbound_certificate.cert").read()

        # Market
        demand = (
            DemandBuilder(requestor)
            .props_from_template(task_package = None)
            .property("golem.srv.comp.payload", base64.b64encode(manifest.encode()).decode())
            .property("golem.srv.comp.payload.sig", signature)
            .property("golem.srv.comp.payload.sig.algorithm", "sha256")
            .property("golem.srv.comp.payload.cert", base64.b64encode(certificate.encode()).decode())
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

        output_file = "output.json"
        output_path = Path(runner.web_root_path) / "upload" / output_file

        exe_script = vm_exe_script(runner, server_addr, output_file)
        print(exe_script)

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
        
        output_text = open(output_path).read()
        logger.info('Received %s', output_text)
        output_json = json.loads(output_text)
        logger.info('Parsed as %s', output_json)
        assert 'Ok' in output_json['roundtrip']
        assert 'Ok' in output_json['many_reqs']
        assert 'Ok' in output_json['iperf3']
        assert 'Ok' in output_json['stress']
