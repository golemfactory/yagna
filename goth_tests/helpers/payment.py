"""Helper functions for easy handling of payments."""

import asyncio
import logging

from dataclasses import dataclass
from datetime import datetime, timezone, timedelta
from typing import List, Optional, Tuple, Dict, Union

from goth.runner.probe import ProviderProbe, RequestorProbe
from goth.runner.probe.rest_client import ya_payment

logger = logging.getLogger("goth.tests.helpers.payment")

global_deposits = []


async def pay_all(
        requestor: RequestorProbe,
        agreements: List[Tuple[str, ProviderProbe]],
):
    """Pay for all Agreements."""
    for agreement_id, provider in agreements:
        await provider.wait_for_invoice_sent()
        invoices = await requestor.gather_invoices(agreement_id)
        assert all(inv.agreement_id == agreement_id for inv in invoices)
        # TODO:
        await requestor.pay_invoices(invoices)
        await provider.wait_for_invoice_paid()


async def accept_debit_notes(
        allocation,
        requestor: RequestorProbe,
        stats: "DebitNoteStats",
):
    ts = datetime.now(timezone.utc)
    logger.info("Listening for debit note events")
    while True:
        try:
            # FIXME: requestor.api.payment.get_debit_note_events returns
            #  instances of 'DebitNoteReceivedEvent', which do not contain
            #  the `eventDate` property
            events = await get_debit_note_events_raw(requestor, ts)
        except Exception as e:
            logger.error("Failed to fetch debit note events: %s", e)
            events = []

        for ev in events:
            debit_note_id = ev.get("debitNoteId")
            event_date = ev.get("eventDate")
            event_type = ev.get("eventType")

            ts = datetime.fromisoformat(event_date.replace("Z", "+00:00"))

            if event_type != "DebitNoteReceivedEvent":
                logger.warning("Invalid debit note event type: %s", event_type)
                continue
            if not (debit_note_id and event_date):
                logger.warning("Empty debit note event: %r", ev)
                continue

            debit_note = await requestor.api.payment.get_debit_note(debit_note_id)
            stats.amount = float(debit_note.total_amount_due)
            amount = str(debit_note.total_amount_due)

            acceptance = ya_payment.Acceptance(
                total_amount_accepted=amount,
                allocation_id=allocation.allocation_id,
            )

            await requestor.api.payment.accept_debit_note(
                debit_note.debit_note_id,
                acceptance,
            )
            stats.accepted += 1

            logger.info(
                "Debit note %s (amount: %s) accepted",
                debit_note.debit_note_id,
                debit_note.total_amount_due,
            )

        if not events:
            await asyncio.sleep(0.5)


async def get_debit_note_events_raw(
        requestor: RequestorProbe, ts: datetime
) -> List[Dict]:
    client = requestor.api.payment.api_client

    path_params = {}
    query_params = {"afterTimestamp": ts}
    header_params = {"Accept": client.select_header_accept(["application/json"])}

    return await client.call_api(
        "/debitNoteEvents",
        "GET",
        path_params,
        query_params,
        header_params,
        response_type="object",
        auth_settings=["app_key"],
        _return_http_data_only=True,
        _preload_content=True,
    )


@dataclass
class DebitNoteStats:
    accepted: int = 0
    amount: float = 0.0


@dataclass
class AllocationCtx:
    requestor: RequestorProbe
    amount: Union[str, float, int]
    _id: Optional[str] = None

    async def __aenter__(self):
        allocation = None
        if global_deposits:
            for global_deposit in global_deposits:
                try:
                    logger.info("Creating allocation for deposit {}".format(global_deposit))

                    allocation_arg = ya_payment.Allocation(
                        allocation_id="",
                        total_amount=str(self.amount),
                        spent_amount=0,
                        remaining_amount=0,
                        make_deposit=True,
                        timestamp=datetime.now(timezone.utc),
                        timeout=datetime.now(timezone.utc) + timedelta(minutes=30),
                        payment_platform=self.requestor.payment_config.platform_string,
                        deposit=global_deposit,
                    )
                    allocation = await self.requestor.api.payment.create_allocation(allocation_arg)
                except Exception as ex:
                    logger.warning("Failed to create allocation for deposit {} - {}".format(global_deposit["id"], ex))
                    continue
        else:
            logger.info("Creating allocation without deposit")

            allocation_arg = ya_payment.Allocation(
                allocation_id="",
                total_amount=str(self.amount),
                spent_amount=0,
                remaining_amount=0,
                make_deposit=True,
                timestamp=datetime.now(timezone.utc),
                timeout=datetime.now(timezone.utc) + timedelta(minutes=30),
                payment_platform=self.requestor.payment_config.platform_string,
                deposit=None,
            )

            allocation = await self.requestor.api.payment.create_allocation(allocation_arg)

        if not allocation:
            raise RuntimeError("Failed to create allocation at all")
        self._id = allocation.allocation_id
        return allocation

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if self._id:
            await self.requestor.api.payment.release_allocation(self._id)
