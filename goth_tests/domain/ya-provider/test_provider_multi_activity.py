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


@pytest.mark.asyncio
async def test_provider_renegotiate_proposal(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Tests providers' ability of renegotiating previously rejected proposal."""

    nodes = [
        {"name": "requestor-1", "type": "Requestor"},
        {"name": "requestor-2", "type": "Requestor"},
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
        requestor1, requestor2 = runner.get_probes(probe_type=RequestorProbe)
        providers = runner.get_probes(probe_type=ProviderProbe)

        def build_demand(requestor):
            return (
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

        async def negotiate_begin(requestor, demand, providers):
            logger.info(
                "%s Negotiating with providers",
                requestor.name,
            )
            for provider in providers:
                await provider.wait_for_offer_subscribed()

            subscription_id, demand = await requestor.subscribe_demand(demand)

            proposals = await requestor.wait_for_proposals(
                subscription_id,
                providers,
                lambda p: p.properties.get("golem.runtime.name") == "wasmtime",
            )
            logger.info("Collected %s proposals", len(proposals))
            return subscription_id, proposals

        async def negotiate_rejection(
            requestor, demand, providers, subscription_id, proposals
        ):
            counter_providers = []
            for proposal in proposals:
                provider = next(p for p in providers if p.address == proposal.issuer_id)
                logger.info(
                    "%s Processing proposal from %s", requestor.name, provider.name
                )

                counter_proposal_id = await requestor.counter_proposal(
                    subscription_id, demand, proposal
                )
                counter_providers.append((counter_proposal_id, provider))
            return counter_providers

        async def renegotiate(requestor, counter_providers, subscription_id):
            logger.info("%s: renegotiate()", requestor.name)
            agreement_providers = []
            for counter_proposal_id, provider in counter_providers:
                logger.info(
                    "%s with %s. p.wait_for_proposal_accepted()",
                    requestor.name,
                    provider.name,
                )
                # await provider.wait_for_proposal_accepted()

                new_proposals = []
                collected_offers = await requestor.api.market.collect_offers(
                    subscription_id
                )
                logger.info("collected offers: %s", collected_offers)
                assert len(collected_offers) == 2
                assert (
                    collected_offers[0].reason.message
                    == "No capacity available. Reached Agreements limit: 1"
                )
                new_proposals.append(collected_offers[1].proposal)

                agreement_id = await requestor.create_agreement(new_proposals[0])
                await requestor.confirm_agreement(agreement_id)
                await provider.wait_for_agreement_approved()
                await requestor.wait_for_approval(agreement_id)
                agreement_providers.append((agreement_id, provider))
            return agreement_providers

        async def negotiate_finalize(
            requestor, demand, providers, subscription_id, proposals
        ):
            logger.info("%s: negotiate_finalize()", requestor.name)
            agreement_providers = []

            for proposal in proposals:
                provider = next(p for p in providers if p.address == proposal.issuer_id)
                logger.info(
                    "%s Processing proposal from %s", requestor.name, provider.name
                )

                counter_proposal_id = await requestor.counter_proposal(
                    subscription_id, demand, proposal
                )
                await provider.wait_for_proposal_accepted()

                new_proposals = await requestor.wait_for_proposals(
                    subscription_id,
                    (provider,),
                    lambda proposal: proposal.prev_proposal_id == counter_proposal_id,
                )

                agreement_id = await requestor.create_agreement(new_proposals[0])
                await requestor.confirm_agreement(agreement_id)
                await provider.wait_for_agreement_approved()
                await requestor.wait_for_approval(agreement_id)
                agreement_providers.append((agreement_id, provider))

            await requestor.unsubscribe_demand(subscription_id)
            logger.info("Got %s agreements", len(agreement_providers))
            return agreement_providers

        async def run(requestor, agreement_providers, second_requestor=None):
            logger.info("%s run()", requestor.name)
            for agreement_id, provider in agreement_providers:
                logger.info(
                    "%s Running activity on %s. agreement_id: %s",
                    requestor.name,
                    provider.name,
                    agreement_id,
                )
                activity_id = await requestor.create_activity(agreement_id)
                await provider.wait_for_exeunit_started()
                if second_requestor is not None:
                    second_requestor()
                await requestor.destroy_activity(activity_id)
                await provider.wait_for_exeunit_finished()

                await requestor.terminate_agreement(agreement_id, None)
                await provider.wait_for_agreement_terminated()

            # Payment
            await pay_all(requestor, agreement_providers)
            logger.info("%s run() -> done", requestor.name)

        demand1 = build_demand(requestor1)
        demand2 = build_demand(requestor2)
        subscription_id1, proposals1 = await negotiate_begin(
            requestor1, demand1, providers
        )
        subscription_id2, proposals2 = await negotiate_begin(
            requestor2, demand2, providers
        )
        agreement_providers1 = await negotiate_finalize(
            requestor1, demand1, providers, subscription_id1, proposals1
        )
        logger.info("agreement_providers1: %s", agreement_providers1)
        # Second requestor will get rejection because of capacity limits (provider already has an agreement with requestor 1)
        counter_providers = await negotiate_rejection(
            requestor2, demand2, providers, subscription_id2, proposals2
        )

        await run(requestor1, agreement_providers1)
        # First requestor terminated agreement, so provider should renegotiate with second requestor
        agreement_providers2 = await renegotiate(
            requestor2, counter_providers, subscription_id2
        )
        logger.info("agreement_providers2: %s", agreement_providers2)
        await run(requestor2, agreement_providers2)
