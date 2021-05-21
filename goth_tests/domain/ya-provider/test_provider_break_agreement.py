"""End to end tests for requesting WASM tasks using goth REST API clients."""

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
from goth.runner.container.payment import PaymentIdPool
from goth.runner.container.yagna import YagnaContainerConfig
from goth.runner.probe import RequestorProbe, ProviderProbe

from goth_tests.helpers.activity import run_activity, wasi_exe_script, wasi_task_package
from goth_tests.helpers.negotiation import negotiate_agreements, DemandBuilder
from goth_tests.helpers.payment import pay_all

logger = logging.getLogger(__name__)


def _topology(assets_path: Path) -> List[YagnaContainerConfig]:
    """Define the topology of the test network."""

    payment_id_pool = PaymentIdPool(key_dir=assets_path / "keys")

    # Nodes are configured to communicate via proxy
    provider_env = node_environment(
        rest_api_url_base=YAGNA_REST_URL.substitute(host=PROXY_HOST),
    )
    provider_env["IDLE_AGREEMENT_TIMEOUT"] = "5s"
    provider_env["DEBIT_NOTE_ACCEPTANCE_DEADLINE"] = "9s"
    provider_env["DEBIT_NOTE_INTERVAL"] = "6"

    requestor_env = node_environment(
        rest_api_url_base=YAGNA_REST_URL.substitute(host=PROXY_HOST),
    )

    provider_volumes = {
        assets_path
        / "provider"
        / "presets.json": "/root/.local/share/ya-provider/presets.json"
    }

    return [
        YagnaContainerConfig(
            name="requestor",
            probe_type=RequestorProbe,
            volumes={assets_path / "requestor": "/asset"},
            environment=requestor_env,
            payment_id=payment_id_pool.get_id(),
        ),
        YagnaContainerConfig(
            name="provider_1",
            probe_type=ProviderProbe,
            environment=provider_env,
            volumes=provider_volumes,
            privileged_mode=True,
        ),
    ]


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
) -> Runner:
    goth_config = load_yaml(common_assets / "goth-config.yml", config_overrides)

    return Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=common_assets / "web-root",
    )


@pytest.mark.asyncio
async def test_provider_idle_agreement(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider breaking idle Agreement.

    Provider is expected to break Agreement in time configured by
    variable: IDLE_AGREEMENT_TIMEOUT, if there are no Activities created.
    """
    runner = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(_topology(common_assets)):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor, runner, wasi_task_package),
            providers,
        )

        # Break after 5s + 3s margin
        await providers[0].wait_for_agreement_broken(r"No activity created", timeout=8)

        await pay_all(requestor, agreement_providers)


@pytest.mark.asyncio
async def test_provider_idle_agreement_after_2_activities(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider breaking idle Agreement after 2 Activities were computed.

    Provider is expected to break Agreement, if no new Activity was created
    after time configured by variable: IDLE_AGREEMENT_TIMEOUT.
    This test checks case, when Requestor already computed some Activities,
    but orphaned Agreement at some point.
    """
    runner = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(_topology(common_assets)):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(
                requestor, runner, wasi_task_package, require_debit_notes=False
            ),
            providers,
        )

        agreement_id, provider = agreement_providers[0]
        for i in range(0, 2):
            logger.info("Running activity %n-th time on %s", i, provider.name)
            await run_activity(
                requestor, provider, agreement_id, wasi_exe_script(runner)
            )

        # Break after 5s + 3s margin
        await providers[0].wait_for_agreement_broken("No activity created", timeout=8)

        await pay_all(requestor, agreement_providers)


@pytest.mark.asyncio
async def test_provider_debit_notes_accept_timeout(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider breaking Agreement if Requestor doesn't accept DebitNotes.

    Requestor is expected to accept DebitNotes in timeout negotiated in Offer.
    """
    runner = _create_runner(common_assets, config_overrides, log_dir)

    async with runner(_topology(common_assets)):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor, runner, wasi_task_package),
            providers,
        )

        agreement_id, provider = agreement_providers[0]

        await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()

        # Wait for first DebitNote sent by Provider.
        await providers[0].wait_for_log(
            r"Debit note \[.*\] for activity \[.*\] sent.", timeout=30
        )

        # Negotiated timeout is 8s. Let's wait with some margin.
        await providers[0].wait_for_agreement_broken(
            "Requestor isn't accepting DebitNotes in time",
            timeout=12,
        )

        await pay_all(requestor, agreement_providers)


@pytest.mark.asyncio
async def test_provider_timeout_unresponsive_requestor(
    common_assets: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test provider breaking Agreement if Requestor doesn't accept DebitNotes.

    If Provider is unable to send DebitNotes for some period of time, he should
    break Agreement. This is separate mechanism from DebitNotes keep alive, because
    here we are unable to send them, so they can't timeout.
    """
    runner = _create_runner(common_assets, config_overrides, log_dir)

    # Stopping container takes a little bit more time, so we must send
    # DebitNote later, otherwise Agreement will be terminated due to
    # not accepting DebitNotes by Requestor.
    topology = _topology(common_assets)
    topology[1].environment["DEBIT_NOTE_INTERVAL"] = "15"

    async with runner(topology):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        providers = runner.get_probes(probe_type=ProviderProbe)

        agreement_providers = await negotiate_agreements(
            requestor,
            build_demand(requestor, runner, wasi_task_package),
            providers,
        )

        agreement_id, provider = agreement_providers[0]

        # Create activity without waiting. Otherwise Provider will manage
        # to send first DebitNote, before we kill Requestor Yagna daemon.
        # loop = asyncio.get_event_loop()
        await requestor.create_activity(agreement_id)

        # Stop Requestor probe. This should kill Yagna Daemon and
        # make Requestor unreachable, so Provider won't be able to send DebitNotes.
        requestor.container.stop()

        # Negotiated timeout is 8s. Let's wait with some margin.
        # await task
        await providers[0].wait_for_agreement_broken(
            "Requestor is unreachable more than",
            timeout=12,
        )

        # Note that Agreement will be broken, but Provider won't be
        # able to terminate it, because other Yagna daemon is unreachable,
        # so Provider will retry terminating in infinity.
