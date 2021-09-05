"""Helper functions for waiting for API events.

Functions that wait for API events must be used with care in test scenarios as they
impose stricter temporal ordering on events than steps referring to logs.

For example, the following two log-related steps:
```
    provider_1.wait_agreement_approved()
    provider_2.wait_agreement_approved()
```
refer to different -- and thus independent -- streams of logs: for provider_1's
agent and for provider_2's agent. Therefore, the steps can be switched:
```
    provider_2.wait_agreement_approved()
    provider_1.wait_agreement_approved()
```
without changing the acceptance criteria for the test in which they occur.

In contrast, API events for all nodes are registered as a single stream, and
therefore steps related to API calls made by different agents cannot be switched:
```
    wait_for_approve_agreement_response(provider_1)
    wait_for_approve_agreement_response(provider_2)
```
requires that the ApproveAgreement call made by provider_1's agent precedes, in the
unified stream of API events, the ApproveAgreement call made by provider_2's agent.
"""
import logging
import time

from goth.runner.probe import Probe
from goth.api_monitor.api_events import (
    APIResponse,
    contains_agreement_terminated_event,
    get_response_json,
    is_approve_agreement,
    is_counter_proposal_offer,
    is_issue_debit_note,
    is_send_debit_note,
)


logger = logging.getLogger("goth.test.api_events")


async def wait_for_counter_proposal_response(probe: Probe, prop_id=None, timeout=None):
    """Wait for the response to CounterProposalOffer call."""

    return await probe.runner.wait_for_api_event(
        is_counter_proposal_offer,
        event_type=APIResponse,
        name="CounterProposalOffer response",
        prop_id=prop_id,
        node_name=probe.name,
        timeout=timeout,
    )


async def wait_for_approve_agreement_response(probe: Probe, agr_id=None, timeout=None):
    """Wait for the response to ApproveAgreement call."""

    return await probe.runner.wait_for_api_event(
        is_approve_agreement,
        event_type=APIResponse,
        name="ApproveAgreement response",
        agr_id=agr_id,
        node_name=probe.name,
        timeout=timeout,
    )


async def wait_for_agreement_terminated_event(probe: Probe, agr_id=None, timeout=None):
    """Wait for the response to AgreementEvents containing AgreementTerminatedEvent."""

    return await probe.runner.wait_for_api_event(
        contains_agreement_terminated_event,
        agr_id=agr_id,
        name="AgreementEvents response with AgreementTerminatedEvent",
        node_name=probe.name,
        timeout=timeout,
    )


async def wait_for_debit_note_sent(probe: Probe, agr_id=None, timeout=None):
    """Wait for the response to IssueDebitNote call."""

    deadline = time.time() + timeout

    while True:
        event, match = await probe.runner.wait_for_api_event(
            is_issue_debit_note,
            event_type=APIResponse,
            name="IssueDebitNote response",
            node_name=probe.name,
            timeout=(deadline - time.time()),
        )
        note = get_response_json(event)
        if not agr_id or note.get("agreementId") == agr_id:
            break

    note_id = note.get("debitNoteId")
    logger.debug("Debit note %s issued, agreement id = %s", note_id, agr_id)

    return await probe.runner.wait_for_api_event(
        is_send_debit_note,
        event_type=APIResponse,
        note_id=note_id,
        name="SendDebitNote response",
        node_name=probe.name,
        timeout=(deadline - time.time()),
    )
