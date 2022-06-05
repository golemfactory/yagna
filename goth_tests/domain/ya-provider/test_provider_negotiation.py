"""Test provider behavior on non various negotiation scenarios."""

import logging
from pathlib import Path
from typing import List, Tuple

import pytest

from goth.address import (
    PROXY_HOST,
    YAGNA_REST_URL,
)
from goth.configuration import load_yaml, Override, Configuration
from goth.node import node_environment
from goth.runner import Runner
from goth.runner.container.payment import PaymentIdPool
from goth.runner.container.yagna import YagnaContainerConfig
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import run_activity, wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import negotiate_agreements, DemandBuilder, negotiate_proposal
from goth_tests.helpers.payment import pay_all
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.breaking-agreement")


def build_demand(
        requestor: RequestorProbe,
        runner: Runner,
        task_package_template: str,
        require_debit_notes=True,
):
    """Simplifies creating demand."""

    task_package = task_package_template.format(
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
    )

    if require_debit_notes:
        demand = demand.property("golem.com.payment.debit-notes.accept-timeout?", 8)
    return demand.build()


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
async def test_provider_on_requestor_rejecting_agreement(
        common_assets: Path,
        config_overrides: List[Override],
        log_dir: Path,
):
    """Test provider breaking idle Agreement.

    Provider is expected to break Agreement in time configured by
    variable: IDLE_AGREEMENT_TIMEOUT, if there are no Activities created.
    """
    runner, config = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)
        assert providers

    for provider in providers:
        await provider.wait_for_offer_subscribed()

    demand = build_demand(requestor, runner, wasi_task_package)
    subscription_id, demand = await requestor.subscribe_demand(demand)

    proposals = await requestor.wait_for_proposals(
        subscription_id,
        providers,
        lambda p: True,
    )
    logger.info("Collected %s proposals", len(proposals))

    agreement_providers = []

    for proposal in proposals:
        provider = next(p for p in providers if p.address == proposal.issuer_id)

        new_proposal = await negotiate_proposal(requestor, demand, provider, proposal, subscription_id)

        agreement_id = await requestor.create_agreement(new_proposal)
        await requestor.confirm_agreement(agreement_id)
        await requestor.reject_agreement(agreement_id)



    await requestor.unsubscribe_demand(subscription_id)
    logger.info("Got %s agreements", len(agreement_providers))


