use super::{MarketProviderFacade, ProviderCollectResult, DemandCriteria};
use { Demand, 
    Offer, 
    Agreement, 
    ScanError, 
    SubscribeError, 
    UnSubscribeError, 
    ProposalError, 
    CollectError, 
    AgreementError };

// Implementation of Golem Dedicated Protocol for Market Matching
pub struct GolemMarketProviderFacade {
    pub state : u32,

}

impl MarketProviderFacade 
 for GolemMarketProviderFacade {
     fn new() -> GolemMarketProviderFacade {
         GolemMarketProviderFacade{ state: 0 }
     }

    fn scan(&self, criteria: DemandCriteria) -> Result<(), ScanError> {
        Result::Ok(())
    }

    fn subscribe(&self, offer: Offer) -> Result<u32, SubscribeError> {
        Result::Ok(0)
    }

    fn unsubscribe(&self, subs_id: u32) -> Result<(), UnSubscribeError> {
        Result::Ok(())
    }

    fn collect(&self, subs_id:u32, max_result: u32, timeout: u32) -> Result<Vec<ProviderCollectResult>, CollectError> {
        let result = vec!();
        Result::Ok(result)
    }

    fn create_proposal(&self, offer : Offer, demand : Demand) -> Result<(), ProposalError> {
        Result::Ok(())
    }

    fn approve_agreement(&self, agreement : Agreement) -> Result<(), AgreementError> {
        Result::Ok(())
    }

    fn reject_agreement(&self, agreement : Agreement) -> Result<(), AgreementError> {
        Result::Ok(())
    }
}
