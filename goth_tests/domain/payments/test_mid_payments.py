"""Tests mid-agreement payments"""

import asyncio
import logging
import pytest

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Tuple

from goth.configuration import load_yaml, Override, Configuration
from goth.runner import Runner
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe
from goth_tests.helpers.payment import accept_debit_notes, pay_all, DebitNoteStats

logger = logging.getLogger("goth.test.mid_payments")

ITERATION_COUNT = 5


@dataclass
class DemandProperties:
    debit_note_interval_sec: int = 2
    payment_timeout_sec: int = 5


def build_demand(
    requestor: RequestorProbe,
    properties: DemandProperties,
):
    return (
        DemandBuilder(requestor)
        .props_from_template(None)
        .property(
            "golem.com.scheme.payu.debit-note.interval-sec?",
            properties.debit_note_interval_sec,
        )
        .property(
            "golem.com.scheme.payu.payment-timeout-sec?", properties.payment_timeout_sec
        )
        .constraints(
            "(&(golem.com.pricing.model=linear)\
                (golem.runtime.name=wasmtime))"
        )
        .build()
    )


def _create_runner(
    config_name: str,
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
) -> Tuple[Runner, Configuration]:
    goth_config = load_yaml(
        Path(__file__).parent / config_name,
        config_overrides,
    )

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )

    return runner, goth_config


async def run_mid_agreement_payments(
    runner: Runner,
    config: Configuration,
    properties: DemandProperties,
):
    """Test mid-agreement payments"""
    ts = datetime.now(timezone.utc)
    amount = 0.0

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor, properties),
            providers,
        )

        stats = DebitNoteStats()
        asyncio.create_task(accept_debit_notes(requestor, stats))

        agreement_id, provider = agreement_providers[0]
        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()

        for i in range(0, ITERATION_COUNT):
            await asyncio.sleep(5)
            payments = await provider.api.payment.get_payments(after_timestamp=ts)
            for payment in payments:
                logger.info("Payment: %r", payment)
                amount += float(payment.amount)
                ts = payment.timestamp if payment.timestamp > ts else ts
            # prevent new debit notes in last iterations
            if i == ITERATION_COUNT - 3:
                await requestor.destroy_activity(activity_id)
                await provider.wait_for_exeunit_finished()
            elif i == ITERATION_COUNT - 2:
                await pay_all(
                    requestor, [(agreement_id, provider)], await_payment=False
                )
                await asyncio.sleep(30)

        assert round(stats.amount, 12) == round(amount, 12)


@pytest.mark.asyncio
async def test_mid_agreement_payments(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    runner, config = _create_runner(
        "goth-config.yml", common_assets, config_overrides, log_dir
    )
    properties = DemandProperties()

    await run_mid_agreement_payments(runner, config, properties)


@pytest.mark.asyncio
async def test_mid_agreement_payments_batching(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    runner, config = _create_runner(
        "goth-batch-config.yml", common_assets, config_overrides, log_dir
    )
    properties = DemandProperties(2, 30)

    await run_mid_agreement_payments(runner, config, properties)
