"""End to end tests for requesting WASM tasks using goth REST API clients."""

import json
import logging
import re
from pathlib import Path
from typing import List, Tuple

import pytest
from ya_activity.exceptions import ApiException

from goth.configuration import load_yaml, Override, Configuration
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import negotiate_agreements, DemandBuilder
from goth_tests.helpers.payment import pay_all
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.multi-activity")


def _create_runner(
    common_assets: Path, config_overrides: List[Override], log_dir: Path
) -> Tuple[Runner, Configuration]:
    goth_config = load_yaml(Path(__file__).parent / "goth-config.yml", config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )
    return runner, goth_config


@pytest.mark.asyncio
async def test_provider_multi_activity(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider handling multiple activities in single Agreement.

    Tests running multiple activities on single Provider.
    In this case Requestor is responsible for terminating Agreement.
    """
    runner, config = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        # Market
        task_package = wasi_task_package.format(
            web_server_addr=runner.host_address, web_server_port=runner.web_server_port
        )

        demand = (
            DemandBuilder(requestor)
            .props_from_template(task_package)
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
        )

        #  Activity
        exe_script = wasi_exe_script(runner)

        for agreement_id, provider in agreement_providers:
            for i in range(0, 3):
                logger.info("Running activity %d-th time on %s", i, provider.name)
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


@pytest.mark.asyncio
async def test_provider_single_simultaneous_activity(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider rejecting second activity if one is already running.

    Provider is expected to reject second activity, if one is already running.
    """
    runner, config = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        # Market
        task_package = wasi_task_package.format(
            web_server_addr=runner.host_address, web_server_port=runner.web_server_port
        )

        demand = (
            DemandBuilder(requestor)
            .props_from_template(task_package)
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
        )

        #  Activity
        agreement_id, provider = agreement_providers[0]

        first_activity_id = await requestor.create_activity(agreement_id)

        # Creation should fail here.
        with pytest.raises(ApiException) as e:
            await requestor.create_activity(agreement_id)

        assert re.search(
            r"terminated. Reason: Only single Activity allowed,"
            r" message: Can't create 2 simultaneous Activities.",
            e.value.body,
        )

        await requestor.destroy_activity(first_activity_id)
        await provider.wait_for_exeunit_finished()

        await requestor.terminate_agreement(agreement_id, None)
        await provider.wait_for_agreement_terminated()
