use futures::Future;

use crate::error::Error;

pub trait ProviderApi {
    fn approve_agreement(&self, agreement_id: &str) -> Box<Future<Item = (), Error = Error>>;
    fn collect(
        &self,
        subscription_id: &str,
        timeout: f32,
        max_events: i64,
    ) -> Box<Future<Item = Vec<::models::ProviderEvent>, Error = Error>>;
    fn create_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
        proposal: ::models::Proposal,
    ) -> Box<Future<Item = String, Error = Error>>;
    fn get_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Box<Future<Item = ::models::AgreementProposal, Error = Error>>;
    fn query_response(
        &self,
        subscription_id: &str,
        query_id: &str,
        property_query_response: ::models::PropertyQueryResponse,
    ) -> Box<Future<Item = (), Error = Error>>;
    fn reject_agreement(&self, agreement_id: &str) -> Box<Future<Item = (), Error = Error>>;
    fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Box<Future<Item = (), Error = Error>>;
    fn subscribe(&self, offer: ::models::Offer) -> Box<Future<Item = String, Error = Error>>;
    fn unsubscribe(&self, subscription_id: &str) -> Box<Future<Item = (), Error = Error>>;
}

impl ProviderApi for ProviderApiClient {
    fn approve_agreement(&self, agreement_id: &str) -> Box<Future<Item = (), Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Post,
            "/agreements/{agreementId}/approve".to_string(),
        )
        .with_path_param("agreementId".to_string(), agreement_id.to_string())
        .returns_nothing()
        .execute(self.configuration.borrow())
    }

    fn collect(
        &self,
        subscription_id: &str,
        timeout: f32,
        max_events: i64,
    ) -> Box<Future<Item = Vec<::models::ProviderEvent>, Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Get,
            "/offers/{subscriptionId}/events".to_string(),
        )
        .with_query_param("timeout".to_string(), timeout.to_string())
        .with_query_param("maxEvents".to_string(), max_events.to_string())
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .execute(self.configuration.borrow())
    }

    fn create_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
        proposal: ::models::Proposal,
    ) -> Box<Future<Item = String, Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Post,
            "/offers/{subscriptionId}/proposals/{proposalId}/offer".to_string(),
        )
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .with_path_param("proposalId".to_string(), proposal_id.to_string())
        .with_body_param(proposal)
        .execute(self.configuration.borrow())
    }

    fn get_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Box<Future<Item = ::models::AgreementProposal, Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Get,
            "/offers/{subscriptionId}/proposals/{proposalId}".to_string(),
        )
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .with_path_param("proposalId".to_string(), proposal_id.to_string())
        .execute(self.configuration.borrow())
    }

    fn query_response(
        &self,
        subscription_id: &str,
        query_id: &str,
        property_query_response: ::models::PropertyQueryResponse,
    ) -> Box<Future<Item = (), Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Post,
            "/offers/{subscriptionId}/propertyQuery/{queryId}".to_string(),
        )
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .with_path_param("queryId".to_string(), query_id.to_string())
        .with_body_param(property_query_response)
        .returns_nothing()
        .execute(self.configuration.borrow())
    }

    fn reject_agreement(&self, agreement_id: &str) -> Box<Future<Item = (), Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Post,
            "/agreements/{agreementId}/reject".to_string(),
        )
        .with_path_param("agreementId".to_string(), agreement_id.to_string())
        .returns_nothing()
        .execute(self.configuration.borrow())
    }

    fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal_id: &str,
    ) -> Box<Future<Item = (), Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Delete,
            "/offers/{subscriptionId}/proposals/{proposalId}".to_string(),
        )
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .with_path_param("proposalId".to_string(), proposal_id.to_string())
        .returns_nothing()
        .execute(self.configuration.borrow())
    }

    fn subscribe(&self, offer: ::models::Offer) -> Box<Future<Item = String, Error = Error>> {
        __internal_request::Request::new(hyper::Method::Post, "/offers".to_string())
            .with_body_param(offer)
            .execute(self.configuration.borrow())
    }

    fn unsubscribe(&self, subscription_id: &str) -> Box<Future<Item = (), Error = Error>> {
        __internal_request::Request::new(
            hyper::Method::Delete,
            "/offers/{subscriptionId}".to_string(),
        )
        .with_path_param("subscriptionId".to_string(), subscription_id.to_string())
        .returns_nothing()
        .execute(self.configuration.borrow())
    }
}
