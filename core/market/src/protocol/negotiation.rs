#![allow(dead_code)]
pub mod error;
pub mod messages;
pub mod provider;
pub mod requestor;

pub mod common {
    use crate::db::model::{Agreement, OwnerType};
    use crate::protocol::negotiation::error::{GsbAgreementError, TerminateAgreementError};
    use crate::protocol::negotiation::messages::{provider, requestor, AgreementTerminated};

    use ya_client::model::market::Reason;
    use ya_core_model::market::BUS_ID;
    use ya_net::{self as net, RemoteEndpoint};
    use ya_service_bus::RpcEndpoint;

    /// Sent to notify other side about termination.
    pub async fn propagate_terminate_agreement(
        agreement: &Agreement,
        reason: Option<Reason>,
    ) -> Result<(), TerminateAgreementError> {
        let msg = AgreementTerminated {
            agreement_id: agreement.id.clone().swap_owner(),
            reason,
        };

<<<<<<< HEAD
        log::debug!(
            "Propagating TerminateAgreement: [{}]. Reason: {:?}",
            &msg.agreement_id,
            &msg.reason
        );
        let service = match agreement.id.clone().owner() {
            OwnerType::Requestor => provider::agreement_addr(BUS_ID),
            OwnerType::Provider => requestor::agreement_addr(BUS_ID),
=======
        log::debug!("Propagating TerminateAgreement: {:?}", msg);
        let (service, sender, receiver) = match agreement.id.clone().owner() {
            OwnerType::Requestor => (
                provider::agreement_addr(BUS_ID),
                agreement.requestor_id,
                agreement.provider_id,
            ),
            OwnerType::Provider => (
                requestor::agreement_addr(BUS_ID),
                agreement.provider_id,
                agreement.requestor_id,
            ),
>>>>>>> 8916e789... agreememt state machine + fix agreement event timestamp + more:
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
