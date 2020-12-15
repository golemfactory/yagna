#![allow(dead_code)]
pub mod error;
pub mod messages;
pub mod provider;
pub mod requestor;

pub mod common {
    use crate::db::model::{Agreement, Owner};
    use crate::protocol::negotiation::error::{GsbAgreementError, TerminateAgreementError};
    use crate::protocol::negotiation::messages::{provider, requestor, AgreementTerminated};

    use ya_client::model::market::Reason;
    use ya_client::model::NodeId;
    use ya_core_model::market::BUS_ID;
    use ya_net::{self as net, RemoteEndpoint};
    use ya_service_bus::RpcEndpoint;

    /// Sent to notify other side about termination.
    pub async fn propagate_terminate_agreement(
        agreement: &Agreement,
        sender: NodeId,
        receiver: NodeId,
        reason: Option<Reason>,
    ) -> Result<(), TerminateAgreementError> {
        let msg = AgreementTerminated {
            agreement_id: agreement.id.clone().swap_owner(),
            reason,
        };

        log::debug!("Propagating TerminateAgreement: {:?}", msg);
        let service = match agreement.id.clone().owner() {
            Owner::Requestor => provider::agreement_addr(BUS_ID),
            Owner::Provider => requestor::agreement_addr(BUS_ID),
        };
        net::from(sender)
            .to(receiver)
            .service(&service)
            .send(msg)
            .await
            .map_err(|e| GsbAgreementError(e.to_string(), agreement.id.clone()))??;
        Ok(())
    }
}
