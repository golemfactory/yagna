"""Tests that zero-amount invoices are settled."""

import logging
from pathlib import Path
from typing import List, Optional

import pytest

from goth.address import (
    PROXY_HOST,
    YAGNA_REST_URL,
)
from goth.configuration import load_yaml, Override
from goth.node import node_environment
from goth.runner import Runner
from goth.runner.probe import RequestorProbe
from ya_payment import InvoiceStatus

from goth_tests.helpers.activity import wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import DemandBuilder, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.zero_amount_txs")


@pytest.mark.asyncio
async def test_zero_amount_invoice(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting WASM tasks with goth REST API client."""

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
        provider = runner.get_probes(probe_type=ProviderProbe)[0]

        # Market
        demand = DemandBuilder(requestor).props_from_template(wasi_task_package).build()

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            [provider],
            lambda p: p.properties.get("golem.runtime.name") == "wasmtime",
        )
        agreement_id = agreement_providers[0][0]

        #  Zero-amount invoice is issued when agreement is terminated
        #  without activity
        await requestor.wait_for_approval(agreement_id)
        await requestor.terminate_agreement(agreement_id, None)

        # Payment

        await provider.wait_for_invoice_sent()
        invoices = await requestor.gather_invoices(agreement_id)
        await requestor.pay_invoices(invoices)
        await provider.wait_for_invoice_paid()

        # verify requestor's invoice is settled
        invoice = (await requestor.gather_invoices(agreement_id))[0]
        assert invoice.amount == "0"
        assert invoice.status == InvoiceStatus.SETTLED
