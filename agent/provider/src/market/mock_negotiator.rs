use super::negotiator::{Negotiator};


pub struct AcceptAllNegotiator {

}


impl Negotiator for AcceptAllNegotiator {

}

impl AcceptAllNegotiator {

    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator{}
    }
}
