use std::str::FromStr;

use ya_client::model::market::Proposal;

use crate::db::model::ProposalId;

pub mod requestor {
    use ya_client::model::market::event::RequestorEvent;
    use ya_client::model::market::Proposal;

    pub fn expect_proposal(events: Vec<RequestorEvent>, i: u8) -> anyhow::Result<Proposal> {
        assert_eq!(events.len(), 1, "{}: Expected one event: {:?}.", i, events);

        Ok(match events[0].clone() {
            RequestorEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }
}

pub mod provider {
    use ya_client::model::market::event::ProviderEvent;
    use ya_client::model::market::Proposal;

    pub fn expect_proposal(events: Vec<ProviderEvent>, i: u8) -> anyhow::Result<Proposal> {
        assert_eq!(events.len(), 1, "{}: Expected one event: {:?}.", i, events);

        Ok(match events[0].clone() {
            ProviderEvent::ProposalEvent { proposal, .. } => proposal,
            _ => anyhow::bail!("Invalid event Type. ProposalEvent expected"),
        })
    }
}

pub trait ClientProposalHelper {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId>;
}

impl ClientProposalHelper for Proposal {
    fn get_proposal_id(&self) -> anyhow::Result<ProposalId> {
        Ok(ProposalId::from_str(self.proposal_id()?)?)
    }
}
