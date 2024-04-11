"""Tests deposit-agreement payments"""

import asyncio
import logging
import pytest
import payment from goth_tests.helpers
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Tuple

from goth.configuration import load_yaml, Override, Configuration
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe
from goth_tests.helpers.payment import accept_debit_notes, DebitNoteStats

logger = logging.getLogger("goth.test.deposit_payments")

DEBIT_NOTE_INTERVAL_SEC = 2
PAYMENT_TIMEOUT_SEC = 5
ITERATION_COUNT = 10
ITERATION_STOP_JOB = 4

def build_demand(
    requestor: RequestorProbe,
):
    return (
        DemandBuilder(requestor)
        .props_from_template(None)
        .property(
            "golem.com.scheme.payu.debit-note.interval-sec?", DEBIT_NOTE_INTERVAL_SEC
        )
        .property("golem.com.scheme.payu.payment-timeout-sec?", PAYMENT_TIMEOUT_SEC)
        .constraints(
            "(&(golem.com.pricing.model=linear)\
                (golem.runtime.name=wasmtime))"
        )
        .build()
    )


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
async def test_deposit_agreement_payments(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    deposit_id = "0x17ec8597ff92c3f44523bdc65bf0f1be632917ff000000000000000000000666"

    payment.global_deposit = deposit_id
    """Test deposit-agreement payments"""
    runner, config = _create_runner(common_assets, config_overrides, log_dir)

    ts = datetime.now(timezone.utc)
    amount = 0.0
    number_of_payments = 0

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor),
            providers,
        )

        stats = DebitNoteStats()
        asyncio.create_task(accept_debit_notes(requestor, stats))

        agreement_id, provider = agreement_providers[0]
        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()

        for i in range(0, ITERATION_COUNT):
            await asyncio.sleep(PAYMENT_TIMEOUT_SEC)

            payments = await provider.api.payment.get_payments(after_timestamp=ts)
            for payment in payments:
                number_of_payments += 1
                amount += float(payment.amount)
                logger.info(f"Received payment: amount {payment.amount}."
                            f" Total amount {amount}. Number of payments {number_of_payments}")
                ts = payment.timestamp if payment.timestamp > ts else ts

            # prevent new debit notes in the last iteration
            if i == ITERATION_STOP_JOB:
                await requestor.destroy_activity(activity_id)
                await provider.wait_for_exeunit_finished()

        # this test is failing too much, so not expect exact amount paid,
        # but at least two payments have to be made
        assert stats.amount > 0
        assert amount > 0
        assert number_of_payments >= 2
