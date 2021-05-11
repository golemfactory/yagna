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
from goth.runner.probe import ProviderProbe, RequestorProbe

from goth_tests.helpers.activity import wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import negotiate_agreements, DemandBuilder
from goth_tests.helpers.payment import pay_all

logger = logging.getLogger("goth.test.multi-activity")


@pytest.mark.asyncio
async def test_provider_multi_activity(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider handling multiple activities in single Agreement."""

    nodes = [
        {"name": "requestor", "type": "Requestor"},
        {"name": "provider-1", "type": "VM-Wasm-Provider", "use-proxy": True},
    ]
    config_overrides.append(("nodes", nodes))

    goth_config = load_yaml(common_assets / "goth-config.yml", config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        # Market
        demand = (
            DemandBuilder(requestor)
            .props_from_template(wasi_task_package)
            .property("golem.srv.caps.multi-activity", True)
            .constraints(
                "(&(golem.com.pricing.model=linear)\
                (golem.srv.caps.multi-activity=true)\
                (golem.runtime.name=wasmtime))"
            )
            .build()
        )

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            providers,
            lambda p: p.properties.get("golem.runtime.name") == "wasmtime",
        )

        #  Activity
        exe_script = wasi_exe_script(runner)

        for agreement_id, provider in agreement_providers:
            for i in range(0, 3):
                logger.info("Running activity %s-th time on %s", i, provider.name)
                activity_id = await requestor.create_activity(agreement_id)
                await provider.wait_for_exeunit_started()
                batch_id = await requestor.call_exec(
                    activity_id, json.dumps(exe_script)
                )
                await requestor.collect_results(
                    activity_id, batch_id, len(exe_script), timeout=30
                )
                await requestor.destroy_activity(activity_id)
                await provider.wait_for_exeunit_finished()

            await requestor.terminate_agreement(agreement_id, None)
            await provider.wait_for_agreement_terminated()

        # Payment
        await pay_all(requestor, agreement_providers)
