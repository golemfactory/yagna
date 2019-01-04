
use { Demand,
    Offer,
    ScanError,
    SubscribeError,
    UnSubscribeError,
    CollectError,
    ProposalError,
    AgreementError
    };

use { MarketRequestorFacade, OfferCriteria };

pub struct GolemMarketRequestorFacade {

}

impl MarketRequestorFacade 
 for GolemMarketRequestorFacade {

    fn new() -> Self {
        GolemMarketRequestorFacade{}
    }

    fn scan(&self, criteria: OfferCriteria) -> Result<u32, ScanError> {
        Result::Ok(0)
    }

    fn subscribe(&self, &demand: Demand) -> Result<u32, SubscribeError> {
        Result::Ok(0)
    }

    fn unsubscribe(&self, subs_id: u32) -> Result<(), UnSubscribeError> {
        Result::Ok(())
    }

    fn collect(&self, subs_id:u32, max_result: u32, timeout: u32) -> Result<Vec<RequestorCollectResult>, CollectError> {
        let result = vec!();
        Result::Ok(result)
    }

    fn create_proposal(&self, &demand : Demand, &offer : Offer) -> Result<(), ProposalError> {
        Result::Ok(())
    }

    fn create_agreement(&self, &demand : Demand, &offer : Offer) -> Result<Agreement, AgreementError> {
        Result::Ok(())
    }

    fn cancel_agreement(&self, &mut agreement : Agreement) -> Result<()), AgreementError> {
        Result::Ok(())
    }

    fn confirm_agreement(&self, &mut agreement : Agreement) -> Result<()), AgreementError> {
        Result::Ok(())
    }

    fn terminate_agreement(&self, &mut agreement : Agreement) -> Result<(), AgreementError> {
        Result::Ok(())
    }

}