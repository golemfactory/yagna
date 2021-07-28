"""This contains extensions of original `Probe` classes from `goth`."""

from goth.runner.probe import ProviderProbe as BaseProviderProbe
from goth.runner.step import step


class ProviderProbe(BaseProviderProbe):
    """Extension of `ProviderProbe` which adds steps related to agent logs."""

    @step()
    async def wait_for_offer_subscribed(self):
        """Wait until the provider agent subscribes to the offer."""
        await self.provider_agent.wait_for_log("Subscribed offer")

    @step()
    async def wait_for_proposal_accepted(self):
        """Wait until the provider agent subscribes to the offer."""
        await self.provider_agent.wait_for_log("Decided to CounterProposal")

    @step()
    async def wait_for_agreement_approved(self):
        """Wait until the provider agent subscribes to the offer."""
        await self.provider_agent.wait_for_log("Decided to ApproveAgreement")

    @step()
    async def wait_for_exeunit_started(self):
        """Wait until the provider agent starts the exe-unit."""
        await self.provider_agent.wait_for_log(
            r"(.*)\[ExeUnit\](.+)Supervisor initialized$"
        )

    @step()
    async def wait_for_exeunit_finished(self):
        """Wait until exe-unit finishes."""
        await self.provider_agent.wait_for_log(
            r"(.*)ExeUnit process exited with status Finished - exit status: 0(.*)"
        )

    @step()
    async def wait_for_agreement_terminated(self):
        """Wait until Agreement will be terminated.

        This can happen for 2 reasons (both caught by this function):
        - Requestor terminates - most common case
        - Provider terminates - it happens for compatibility with previous
        versions of API without `terminate` endpoint implemented. Moreover
        Provider can terminate, because Agreements condition where broken.
        """
        await self.provider_agent.wait_for_log(r"Agreement \[.*\] terminated by")

    @step()
    async def wait_for_agreement_cleanup(self):
        """Wait until Provider will cleanup all allocated resources.

        This can happen before or after Agreement terminated log will be printed.
        """
        await self.provider_agent.wait_for_log(r"Agreement \[.*\] cleanup finished.")

    @step()
    async def wait_for_invoice_sent(self):
        """Wait until the invoice is sent."""
        await self.provider_agent.wait_for_log("Invoice (.+) sent")

    @step(default_timeout=300)
    async def wait_for_invoice_paid(self):
        """Wait until the invoice is paid."""
        await self.provider_agent.wait_for_log("Invoice .+? for agreement .+? was paid")

    @step()
    async def wait_for_agreement_broken(self, reason: str):
        """Wait until Provider will break Agreement."""
        pattern = rf"Breaking agreement .*, reason: {reason}"
        await self.provider_agent.wait_for_log(pattern)

    @step()
    async def wait_for_log(self, pattern: str):
        """Wait for specific log."""
        await self.provider_agent.wait_for_log(pattern)
