use crate::config::presets::json_decimal;
use crate::market::negotiator::factory::AgreementExpirationNegotiatorConfig;
use crate::market::negotiator::{NegotiationResult, NegotiatorComponent, ProposalView};
use bigdecimal::BigDecimal;
use ya_agreement_utils::Error;

pub struct PriceNego {}

static PRICE_PROPERTY: &str = "/golem/com/pricing/model/linear/coeffs";

fn prices(proposal: &ProposalView) -> Result<Vec<BigDecimal>, Error> {
    let value = proposal
        .pointer(PRICE_PROPERTY)
        .ok_or_else(|| Error::NoKey(PRICE_PROPERTY.to_string()))?;

    json_decimal::vec_from_value(value)
        .map_err(|error| Error::InvalidValue(format!("{PRICE_PROPERTY}: {error}")))
}

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
        if let (Ok(demand_prices), Ok(offer_prices)) = (prices(demand), prices(&offer)) {
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
}
