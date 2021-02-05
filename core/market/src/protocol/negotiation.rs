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
    use ya_core_model::market::BUS_ID;
    use ya_net::{self as net, RemoteEndpoint};
    use ya_service_bus::RpcEndpoint;

    use chrono::NaiveDateTime;

    /// Sent to notify other side about termination.
    pub async fn propagate_terminate_agreement(
        agreement: &Agreement,
        reason: Option<Reason>,
        signature: String,
        timestamp: NaiveDateTime,
    ) -> Result<(), TerminateAgreementError> {
        let msg = AgreementTerminated {
            agreement_id: agreement.id.clone().swap_owner(),
            reason,
            signature,
            termination_ts: timestamp,
        };

        log::debug!(
            "Propagating TerminateAgreement: [{}]. Reason: {:?}",
            &msg.agreement_id,
            &msg.reason
        );

        let (service, sender, receiver) = match agreement.id.clone().owner() {
            Owner::Requestor => (
                provider::agreement_addr(BUS_ID),
                agreement.requestor_id,
                agreement.provider_id,
            ),
            Owner::Provider => (
                requestor::agreement_addr(BUS_ID),
                agreement.provider_id,
                agreement.requestor_id,
            ),
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
