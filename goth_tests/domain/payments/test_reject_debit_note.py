"""Tests debit-note rejection and provider notification."""

import asyncio
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Tuple

import pytest
from goth.configuration import Configuration, Override, load_yaml
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.payment import get_debit_note_events_raw
from goth_tests.helpers.probe import ProviderProbe


DEBIT_NOTE_INTERVAL_SEC = 2


def _create_runner(
    common_assets: Path, config_overrides: List[Override], log_dir: Path
) -> Tuple[Runner, Configuration]:
    config = load_yaml(Path(__file__).parent / "goth-config.yml", config_overrides)
    return (
        Runner(
            base_log_dir=log_dir,
            compose_config=config.compose_config,
            web_root_path=common_assets / "web-root",
        ),
        config,
    )


def _build_demand(requestor: RequestorProbe):
    return (
        DemandBuilder(requestor)
        .props_from_template(None)
        .property(
            "golem.com.scheme.payu.debit-note.interval-sec?",
            DEBIT_NOTE_INTERVAL_SEC,
        )
        .constraints(
            "(&(golem.com.pricing.model=linear)(golem.runtime.name=wasmtime))"
        )
        .build()
    )


async def _reject_debit_note(requestor: RequestorProbe, debit_note_id: str):
    client = requestor.api.payment.api_client
    await client.call_api(
        f"/debitNotes/{debit_note_id}/reject",
        "POST",
        {},
        {},
        {
            "Accept": client.select_header_accept(["application/json"]),
            "Content-Type": client.select_header_content_type(["application/json"]),
        },
        body={
            "rejectionReason": "INCORRECT_AMOUNT",
            "totalAmountAccepted": "0",
            "message": "unexpected usage",
        },
        response_type=None,
        auth_settings=["app_key"],
        _return_http_data_only=True,
        _preload_content=True,
    )


@pytest.mark.asyncio
async def test_reject_debit_note_notifies_provider(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    runner, config = _create_runner(common_assets, config_overrides, log_dir)
    after = datetime.now(timezone.utc)

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        agreement_id, provider = (
            await negotiate_agreements(requestor, _build_demand(requestor), providers)
        )[0]

        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()

        for _ in range(30):
            events = await get_debit_note_events_raw(requestor, after)
            received = next(
                (event for event in events if event["eventType"] == "DebitNoteReceivedEvent"),
                None,
            )
            if received:
                break
            await asyncio.sleep(0.5)
        else:
            pytest.fail("requestor did not receive a debit note")

        debit_note_id = received["debitNoteId"]
        await _reject_debit_note(requestor, debit_note_id)

        for _ in range(30):
            events = await get_debit_note_events_raw(provider, after)
            rejection = next(
                (
                    event
                    for event in events
                    if event["debitNoteId"] == debit_note_id
                    and event["eventType"] == "DebitNoteRejectedEvent"
                ),
                None,
            )
            if rejection:
                break
            await asyncio.sleep(0.5)
        else:
            pytest.fail("provider was not notified about the debit-note rejection")

        assert rejection["rejectionReason"] == "INCORRECT_AMOUNT"
        assert rejection["totalAmountAccepted"] == "0"

        await requestor.destroy_activity(activity_id)
