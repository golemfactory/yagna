use crate::market::termination_reason::BreakReason;

/// Result of agreement execution.
#[derive(Clone)]
pub enum AgreementResult {
    /// Failed to approve agreement. (Agreement even wasn't created)
    ApprovalFailed,
    /// Agreement was finished with success after first Activity.
    ClosedByUs,
    /// Agreement was finished with success by Requestor.
    ClosedByRequestor,
    /// Agreement was broken by us.
    Broken { reason: BreakReason },
}
