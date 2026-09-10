"""Goth runner extensions used by Yagna integration tests."""

import sys
from pathlib import Path

from goth.runner import Runner as GothRunner
from goth.runner.probe import ProviderProbe


TRANSFER_ALLOWED_IPS_ENV = "YA_TRANSFER_ALLOWED_IPS"


class Runner(GothRunner):
    """Configure transfer access from the Docker network discovered by Goth."""

    def _create_probes(self, scenario_dir: Path) -> None:
        # Goth obtains this address from Docker's network IPAM configuration.
        # Keep the transfer sandbox allowlist in sync instead of duplicating a
        # gateway address in each YAML topology.
        if sys.platform == "linux":
            allowed_host = f"{self.host_address}/32"
            for config in self._topology:
                if issubclass(config.probe_type, ProviderProbe):
                    config.environment[TRANSFER_ALLOWED_IPS_ENV] = allowed_host

        super()._create_probes(scenario_dir)
