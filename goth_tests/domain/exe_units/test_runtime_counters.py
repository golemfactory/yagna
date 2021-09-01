import json
import logging
from pathlib import Path
from typing import List, Tuple

import pytest

from goth.configuration import load_yaml, Override, Configuration
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.negotiation import negotiate_agreements, DemandBuilder
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.runtime.custom-counters")


def build_demand(
    requestor: RequestorProbe,
    require_debit_notes=True,
):
    """Simplifies creating demand."""

    demand = (
        DemandBuilder(requestor)
        .props_from_template("")
        .property("golem.srv.caps.multi-activity", True)
        .constraints(
            "(&(golem.com.pricing.model=linear)\
                (golem.srv.caps.multi-activity=true)\
                (golem.runtime.name=test-counters))"
        )
    )

    if require_debit_notes:
        demand = demand.property("golem.com.payment.debit-notes.accept-timeout?", 8)
    return demand.build()


def _exe_script(duration: float = 3.0):
    return [
        {"deploy": {}},
        {"start": {"args": []}},
        {
            "run": {
                "entry_point": "sleep",
                "args": [f"{duration * 1000}"],
            }
        },
        {
            "run": {
                "entry_point": "stop",
                "args": [],
            }
        },
    ]


def _create_runner(
    common_assets: Path, config_overrides: List[Override], log_dir: Path
) -> Tuple[Runner, Configuration]:
    goth_config = load_yaml(
        Path(__file__).parent / "goth-config.yml",
        config_overrides,
    )

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    return runner, goth_config


@pytest.mark.asyncio
async def test_custom_runtime_counter(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider breaking idle Agreement.

    Provider is expected to break Agreement in time configured by
    variable: IDLE_AGREEMENT_TIMEOUT, if there are no Activities created.
    """
    runner, config = _create_runner(common_assets, config_overrides, log_dir)
    counter_name = "golem.usage.custom.counter"
    exe_script = _exe_script()

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor),
            providers,
        )

        agreement_id, provider = agreement_providers[0]
        agreement = await requestor.api.market.get_agreement(agreement_id)
        usage_vector = agreement.offer.properties["golem.com.usage.vector"]

        assert counter_name in usage_vector
        counter_idx = usage_vector.index(counter_name)

        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()

        batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
        await requestor.collect_results(
            activity_id, batch_id, len(exe_script), timeout=10
        )

        await requestor.destroy_activity(activity_id)
        await provider.wait_for_exeunit_finished()

        debit_notes = await requestor.api.payment.get_debit_notes()
        for debit_note in debit_notes:
            assert len(debit_note.usage_counter_vector) == len(usage_vector)
        assert debit_notes[-1].usage_counter_vector[counter_idx] > 0
