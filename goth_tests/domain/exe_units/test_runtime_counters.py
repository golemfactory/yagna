import asyncio
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
FINAL_DEBIT_NOTE_TIMEOUT = 30.0


def build_demand(
    requestor: RequestorProbe,
):
    """Simplifies creating demand."""

    return (
        DemandBuilder(requestor)
        .props_from_template(None)
        .property("golem.srv.caps.multi-activity", True)
        .constraints(
            "(&(golem.com.pricing.model=linear)\
                (golem.srv.caps.multi-activity=true)\
                (golem.runtime.name=test-counters))"
        )
        .build()
    )


def _exe_script(duration: int = 10000):
    return [
        {"deploy": {}},
        {"start": {"args": []}},
        {
            "run": {
                "entry_point": "sleep",
                "args": [f"{duration}"],
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


async def _wait_for_new_debit_note(
    requestor: RequestorProbe,
    activity_id: str,
    existing_debit_note_ids: set[str],
):
    async def poll():
        while True:
            debit_notes = await requestor.api.payment.get_debit_notes()
            new_debit_notes = [
                debit_note
                for debit_note in debit_notes
                if debit_note.activity_id == activity_id
                and debit_note.debit_note_id not in existing_debit_note_ids
            ]
            if new_debit_notes:
                return max(new_debit_notes, key=lambda debit_note: debit_note.timestamp)

            await asyncio.sleep(0.5)

    try:
        return await asyncio.wait_for(poll(), timeout=FINAL_DEBIT_NOTE_TIMEOUT)
    except asyncio.TimeoutError:
        raise AssertionError(
            f"Final debit note for activity {activity_id} was not received "
            f"within {FINAL_DEBIT_NOTE_TIMEOUT} seconds"
        ) from None


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
        await requestor.collect_results(activity_id, batch_id, len(exe_script))

        debit_notes = await requestor.api.payment.get_debit_notes()
        existing_debit_note_ids = {
            debit_note.debit_note_id
            for debit_note in debit_notes
            if debit_note.activity_id == activity_id
        }

        await requestor.destroy_activity(activity_id)
        await provider.wait_for_exeunit_finished()

        logger.info("waiting for final debit note to be received")
        last_debit_note = await _wait_for_new_debit_note(
            requestor,
            activity_id,
            existing_debit_note_ids,
        )
        logger.info("last debit note: %r", last_debit_note)

        assert len(last_debit_note.usage_counter_vector) == len(usage_vector)
        assert last_debit_note.usage_counter_vector[counter_idx] > 0
