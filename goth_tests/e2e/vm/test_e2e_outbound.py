"""End to end tests for requesting VM tasks using goth REST API client."""

import json
import logging
import os
import base64
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
from goth.runner.probe import RequestorProbe

from goth_tests.helpers.activity import vm_exe_script_outbound
from goth_tests.helpers.negotiation import DemandBuilder, PayloadManifest, negotiate_agreements
from goth_tests.helpers.probe import ProviderProbe

logger = logging.getLogger("goth.test.e2e_outbound")


@pytest.mark.asyncio
async def test_e2e_outbound(
    common_assets: Path,
    default_config: Path,
    config_overrides: List[Override],
    log_dir: Path,
):
    """Test successful flow requesting a Blender task with goth REST API client."""

    # Test external api request just one Requestor and one Provider
    nodes = [
        {"name": "requestor", "type": "Requestor"},
        {"name": "provider-1", "type": "VM-Wasm-Provider", "use-proxy": True},
    ]
    config_overrides.append(("nodes", nodes))

    goth_config = load_yaml(default_config, config_overrides)

    runner = Runner(
        base_log_dir=log_dir,
        compose_config=goth_config.compose_config,
        web_root_path=Path(__file__).parent / "assets",
    )

    async with runner(goth_config.containers):
        requestor = runner.get_probes(probe_type=RequestorProbe)[0]
        provider = runner.get_probes(probe_type=ProviderProbe)[0]

        with open(f"{runner.web_root_path}/outbound_manifest.json") as f:
            manifest = f.read()

        # Market
        payload_manifest = PayloadManifest(
            payload=base64.b64encode(manifest.encode('ascii')).decode("utf-8"),
            payload_sig="R4dWXuS3ll52YDBYQd5h55YIURnKWGMXwv51Yzp8ntqtbkJF6WjJGXcQ977KhnoSYHJOSXJml+K+KoPnJfj7cJ1hxWD99dl8KZT1qgvG1GgUDVD/Wv7+CXhfBY5aEHai/g16f31QunOgZ7yvsqgkBDn270fucxjDjEkvkgf0MRTo7gIiOBzNnb26suLKX+lc88O9MANITTQ6Wc3fUIuInfmylqwISOBvE1fpy1+Dya++FlknO0CpdCVJiBwSKOhVXeL6YFtCLySmcZEQia0w5ivwMp1biHqb3PWDz/pNFo4u2ZGzNc1rMIow0hKtIyAUU1AT9M4f5cfRRH6PwO4/9w==",
            payload_sig_alg="sha256",
            cert="LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCk1JSUU2RENDQTlDZ0F3SUJBZ0lDRUFBd0RRWUpLb1pJaHZjTkFRRUxCUUF3Z1lNeEN6QUpCZ05WQkFZVEFsQk0KTVJNd0VRWURWUVFJREFwTllXeHZjRzlzYzJ0aE1ROHdEUVlEVlFRS0RBWkdiMjhnUTI4eEZUQVRCZ05WQkFzTQpERVp2YnlCSmJuUmxjaUJJVVRFU01CQUdBMVVFQXd3SlJtOXZJRWx1ZEdWeU1TTXdJUVlKS29aSWh2Y05BUWtCCkZoUnZabVpwWTJWQWFXNTBaWEl1Wm05dkxtTnZiVEFnRncweU1qQTRNVEF4TWpBMU1qSmFHQTh5TVRJeU1EY3gKTnpFeU1EVXlNbG93Z1k4eEN6QUpCZ05WQkFZVEFrTmFNUkF3RGdZRFZRUUlEQWRDYjJobGJXbGhNUTh3RFFZRApWUVFIREFaUWNtRm5kV1V4RXpBUkJnTlZCQW9NQ2tadmJ5QlNaWEVnUTI4eEV6QVJCZ05WQkFzTUNrWnZieUJTClpYRWdTRkV4RURBT0JnTlZCQU1NQjBadmJ5QlNaWEV4SVRBZkJna3Foa2lHOXcwQkNRRVdFbTltWm1salpVQnkKWlhFdVptOXZMbU52YlRDQ0FTSXdEUVlKS29aSWh2Y05BUUVCQlFBRGdnRVBBRENDQVFvQ2dnRUJBTmJFdTNBUgoxVjdjTnUxejFOL0NxUWE3d2ZrUmZ0TUl3RGo4WTIyQ0lpNm04N0lBN3NtRTRWNHhNZUFrSjBHS3RPMldaSW5tCnJOVkRKUG9ldXFjS1YrdGszd0xTdFg4dXQyTmw4dS8rMUNyVHRyV0I1Wi9ONk82NzAxdlVVMzc1RFdWeXJHK2gKWjl6Y21iTFZBZGkvYXpCdVVXYnFjVS9ObnhCeUpPRnlXWEJQTXBCR0tBY09KVGduaVFRMk5ZVVJXaDVmclVvMAp3MXh3Z3ZpWms1OTZrSG8xdDU3a2t4M3E5ZVJ4eWdwS21uS21NZ1I5cFlrOFpTWnJPdFcrYzVheUVhdTJUbVNRCmorT0E5WnUzRWM5M0l5UzRiSWJINlFDaG1CMEkrYWk4VTk3SExhUjBVZUVHZWVpUVo5bktseUltT3lJcHV2dXcKUGV2ckdYZnFVRGhlVGNVQ0F3RUFBYU9DQVZRd2dnRlFNQWtHQTFVZEV3UUNNQUF3RVFZSllJWklBWWI0UWdFQgpCQVFEQWdaQU1ETUdDV0NHU0FHRytFSUJEUVFtRmlSUGNHVnVVMU5NSUVkbGJtVnlZWFJsWkNCVFpYSjJaWElnClEyVnlkR2xtYVdOaGRHVXdIUVlEVlIwT0JCWUVGS0l1RkkrVHBTWUhzaXQ5bUhxY3pMOGExMitnTUlHMkJnTlYKSFNNRWdhNHdnYXVBRlBDUytmMWNpWUlzV24vdEI4eEZZa0xjZ1N2SW9ZR09wSUdMTUlHSU1Rc3dDUVlEVlFRRwpFd0pRVERFVE1CRUdBMVVFQ0F3S1RXRnNiM0J2YkhOcllURVBNQTBHQTFVRUJ3d0dTM0poYTI5M01ROHdEUVlEClZRUUtEQVpHYjI4Z1EyOHhFakFRQmdOVkJBc01DVVp2YnlCRGJ5QklVVEVQTUEwR0ExVUVBd3dHUm05dklFTnYKTVIwd0d3WUpLb1pJaHZjTkFRa0JGZzV2Wm1acFkyVkFabTl2TG1OdmJZSUNFQUF3RGdZRFZSMFBBUUgvQkFRRApBZ1dnTUJNR0ExVWRKUVFNTUFvR0NDc0dBUVVGQndNQk1BMEdDU3FHU0liM0RRRUJDd1VBQTRJQkFRQ0toL2hlCk5IbEZQeXZXWlpYNDhlQ2Q1NEZGQ3NrNHpuUitvQlkxYVcxancrN0FGcUt4cUVmZWNkSlIrdUMrczBLVmNhUlEKWjZGcTUwaWNURUs3K1RmNHM0TGMvLytOSi92UEIxd1ZrNW1Rd3Zyb1RTcWZuOXRDdWNFL3gwa0FJSS80QmpZegpKUUNyZnlyVWlDbVBoME1vQTZ3Q0RiNlpMQ0M3TlFmVkFuN2RZNHVvWjNObDR3TW9IOGpSVWhYaHRKNjcwT3pGClpCc2lzSHBKS3RSaWJkOUNvY3I0dHdvTUFNbndpL3k0NWY4SC8yVm9FdnNKdzBDY00wK3FQUWVqSmNyMXh5TVAKMWZMQmNNTHlpeGNQNWYxLzlzWVRhQWN6TkU0eU1ReE9DS2hnZnBXcGE1Y2hVU2F6R2RnalBmNERaL0FSQSswWAowMmMrdGhMVStXMUdkbUhTCi0tLS0tRU5EIENFUlRJRklDQVRFLS0tLS0K",
        )

        demand = (
            DemandBuilder(requestor)
            .props_from_template(task_package=None, payload_manifest=payload_manifest)
            .constraints("(&(golem.runtime.name=vm))")
            .build()
        )

        agreement_providers = await negotiate_agreements(
            requestor,
            demand,
            [provider],
            lambda proposal: proposal.properties.get("golem.runtime.name") == "vm",
        )

        agreement_id, provider = agreement_providers[0]

        # Activity

        output_file = "output.txt"
        output_path = Path(runner.web_root_path) / "upload" / output_file

        exe_script = vm_exe_script_outbound(runner, output_file)

        num_commands = len(exe_script)

        logger.info("Running activity on %s", provider.name)
        activity_id = await requestor.create_activity(agreement_id)
        await provider.wait_for_exeunit_started()
        batch_id = await requestor.call_exec(activity_id, json.dumps(exe_script))
        await requestor.collect_results(
            activity_id, batch_id, num_commands, timeout=300
        )
        await requestor.destroy_activity(activity_id)
        await provider.wait_for_exeunit_finished()

        assert output_path.is_file()
        assert len(output_path.read_text()) > 0

        # Payment
        # Todo probably unnecessary

        await provider.wait_for_invoice_sent()
        invoices = await requestor.gather_invoices(agreement_id)
        assert all(inv.agreement_id == agreement_id for inv in invoices)
        # TODO:
        await requestor.pay_invoices(invoices)
        await provider.wait_for_invoice_paid()
