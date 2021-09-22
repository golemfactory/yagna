


use ya_agreement_utils::{OfferDefinition};


use crate::market::negotiator::factory::AgreementExpirationNegotiatorConfig;
use crate::market::negotiator::{
    AgreementResult, NegotiationResult, NegotiatorComponent, ProposalView,
};

pub struct PriceNego {}

static PRICE_PROPERTY: &'static str = "/golem/com/pricing/model/linear/coeffs";

impl PriceNego {
    pub fn new(_config: &AgreementExpirationNegotiatorConfig) -> anyhow::Result<Self> {
        Ok(PriceNego {})
    }
}

impl NegotiatorComponent for PriceNego {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if let (Ok(demand_prices), Ok(offer_prices)) = (
            demand.pointer_typed::<Vec<f64>>(PRICE_PROPERTY),
            offer.pointer_typed::<Vec<f64>>(PRICE_PROPERTY),
        ) {
            if demand_prices == offer_prices {
                return Ok(NegotiationResult::Ready { offer });
            }
            if demand_prices.len() != offer_prices.len() {
                return Ok(NegotiationResult::Reject {
                    message: "invalid price vector".to_string(),
                    is_final: false,
                });
            }
            if demand_prices
                .iter()
                .zip(&offer_prices)
                .all(|(dp, op)| dp >= op)
            {
                if let Some(p) = offer.pointer_mut(PRICE_PROPERTY) {
                    *p = demand.pointer(PRICE_PROPERTY).unwrap().clone();
                }
                Ok(NegotiationResult::Negotiating { offer })
            } else {
                Ok(NegotiationResult::Reject {
                    message: format!("{:?} < {:?}", demand_prices, offer_prices),
                    is_final: true,
                })
            }
        } else {
            Ok(NegotiationResult::Ready { offer })
        }
    }

    fn fill_template(&mut self, template: OfferDefinition) -> anyhow::Result<OfferDefinition> {
        Ok(template)
    }

    fn on_agreement_terminated(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_agreement_approved(&mut self, _agreement_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
