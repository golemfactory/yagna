use { Demand,
    Offer,
    Agreement,
    ScanError,
    SubscribeError,
    UnSubscribeError,
    CollectError,
    ProposalError,
    AgreementError
    };


pub struct OfferCriteria {

}

pub enum RequestorCollectResult {
    Offer(super::Offer),
}

pub trait MarketRequestorFacade {
    fn new()
        -> Self;

    fn scan(&self, criteria: OfferCriteria) 
        -> Result<u32, ScanError>;

    fn subscribe(&self, demand: &Demand) 
        -> Result<u32, SubscribeError>;

    fn unsubscribe(&self, subs_id: u32) 
        -> Result<(), UnSubscribeError>;

    fn collect(&self, subs_id:u32, max_result: u32, timeout: u32) 
        -> Result<Vec<RequestorCollectResult>, CollectError>;

    fn create_proposal(&self, demand : &Demand, offer : &Offer) 
        -> Result<(), ProposalError>;

    fn create_agreement(&self, demand : &Demand, offer : &Offer) 
        -> Result<Agreement, AgreementError>;

    fn cancel_agreement(&self, agreement : &mut Agreement) 
        -> Result<(), AgreementError>;
    
    fn confirm_agreement(&self, agreement : &mut Agreement) 
        -> Result<(), AgreementError>;

    fn terminate_agreement(&self, agreement : &mut Agreement) 
        -> Result<(), AgreementError>;

}