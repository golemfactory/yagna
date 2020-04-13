pub mod market_impl;

use {
    Agreement, AgreementError, CollectError, Demand, Offer, ProposalError, ScanError,
    SubscribeError, UnSubscribeError,
};

pub struct DemandCriteria {}

pub enum ProviderCollectResult {
    Demand(super::Demand),
}

pub trait MarketProviderFacade {
    fn new() -> Self;

    fn scan(&self, criteria: DemandCriteria) -> Result<(), ScanError>;

    fn subscribe(&self, offer: Offer) -> Result<u32, SubscribeError>;

    fn unsubscribe(&self, subs_id: u32) -> Result<(), UnSubscribeError>;

    fn collect(
        &self,
        subs_id: u32,
        max_result: u32,
        timeout: u32,
    ) -> Result<Vec<ProviderCollectResult>, CollectError>;

    fn create_proposal(&self, offer: Offer, demand: Demand) -> Result<(), ProposalError>;

    fn approve_agreement(&self, agreement: Agreement) -> Result<(), AgreementError>;

    fn reject_agreement(&self, agreement: Agreement) -> Result<(), AgreementError>;
}
