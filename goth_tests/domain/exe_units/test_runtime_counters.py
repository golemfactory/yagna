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
):
    """Simplifies creating demand."""

    return (
        DemandBuilder(requestor)
        .props_from_template(None)
        .property("golem.srv.caps.multi-activity", True)
        .property("golem.com.payment.debit-notes.accept-timeout?", 8)
        .constraints(
            "(&(golem.com.pricing.model=linear)\
                (golem.srv.caps.multi-activity=true)\
                (golem.runtime.name=test-counters))"
        )
        .build()
    )


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
    """Test custom counters provided by the test runtime.

    The final debit note is expected to contain a non-zero custom counter value.
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
        logger.info("usage vector: %r", usage_vector)

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

        logger.info("waiting for last debit note to be send")
        await provider.provider_agent.wait_for_log(r"(.*)Sending debit note(.*)")
        logger.info("waiting for last debit note to be received")
        await requestor.container.logs.wait_for_entry(r"(.*)DebitNote \[(.+)\] received from node(.*)")

        debit_notes = await requestor.api.payment.get_debit_notes()
        last_debit_note = debit_notes[-1]
        logger.info("last debit note: %r", last_debit_note)

        assert len(last_debit_note.usage_counter_vector) == len(usage_vector)
        assert last_debit_note.usage_counter_vector[counter_idx] > 0
