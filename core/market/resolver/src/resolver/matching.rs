use super::error::MatchError;
use super::expression::{Expression, ResolveResult};
use super::prepare::{PreparedDemand, PreparedOffer};
use super::properties::PropertyRef;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchResult<'a> {
    True,
    False(Vec<&'a PropertyRef>, Vec<&'a PropertyRef>),
    Undefined(
        (Vec<&'a PropertyRef>, Expression),
        (Vec<&'a PropertyRef>, Expression),
    ),
    Err(MatchError),
}

pub fn match_weak<'a>(
    demand: &'a PreparedDemand,
    offer: &'a PreparedOffer,
) -> Result<MatchResult<'a>, MatchError> {
    log::trace!("Demand: {:?}", demand);
    log::trace!("Offer: {:?}", offer);

    // Demand constraints reference Offer properties and vice versa. Naming
    // each normalized result after the side that supplies the properties keeps
    // the public mismatch tuple symmetric and unambiguous.
    let from_offer = normalize(
        demand.constraints.resolve(&offer.properties),
        "Demand constraints",
    )?;
    let from_demand = normalize(
        offer.constraints.resolve(&demand.properties),
        "Offer constraints",
    )?;

    log::trace!("Demand constraints with Offer properties: {:?}", from_offer);
    log::trace!(
        "Offer constraints with Demand properties: {:?}",
        from_demand
    );

    if from_offer.undefined || from_demand.undefined {
        Ok(MatchResult::Undefined(
            (from_offer.refs, from_offer.residual),
            (from_demand.refs, from_demand.residual),
        ))
    } else if from_offer.value && from_demand.value {
        Ok(MatchResult::True)
    } else {
        Ok(MatchResult::False(from_offer.refs, from_demand.refs))
    }
}

#[derive(Debug)]
struct NormalizedResult<'a> {
    value: bool,
    undefined: bool,
    refs: Vec<&'a PropertyRef>,
    residual: Expression,
}

fn normalize<'a>(
    result: ResolveResult<'a>,
    constraint_side: &str,
) -> Result<NormalizedResult<'a>, MatchError> {
    match result {
        ResolveResult::True => Ok(NormalizedResult {
            value: true,
            undefined: false,
            refs: Vec::new(),
            residual: Expression::Empty(true),
        }),
        ResolveResult::False(refs, residual) => Ok(NormalizedResult {
            value: false,
            undefined: false,
            refs,
            residual,
        }),
        ResolveResult::Undefined(refs, residual) => Ok(NormalizedResult {
            value: false,
            undefined: true,
            refs,
            residual,
        }),
        ResolveResult::Err(error) => Err(MatchError::new(format!(
            "Error resolving {constraint_side}: {error}"
        ))),
    }
}
