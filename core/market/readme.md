# Decentralized Market for Yagna (Mk1)
The Yagna Market is a core component of the Yagna Network, which enables
computational Offers and Demands circulation. The Market is open for all
entities willing to buy computations (Demands) or monetize computational
resources (Offers).

This implementation conforms with Cabability Level 1 of the 
[Market API specification](
https://docs.google.com/document/d/1Zny_vfgWV-hcsKS7P-Kdr3Fb0dwfl-6T_cYKVQ9mkNg/edit#heading=h.8anq3nlk2en7
) which means support for the three basic phases of the market interaction:
[Discovery](#discovery-phase), [Negotiation](#negotiation-phase) and [Agreement](#agreement-phase).

## Yagna Market API
The Yagna Market API is the entry to the Yagna Market through which
Requestors and Providers  can publish their Demands and Offers
respectively, find matching counterparty, conduct negotiations
and make an agreement.

Each of the two roles: Requestors and Providers have their own
interface in the Market API.

Within the [client library crate](https://github.com/golemfactory/ya-client)
you can find Market API typesafe bindings for Rust.

## Market Interaction
Market interaction is divided into tree phases described below.

### Discovery Phase
Users are joining the Yagna Network by publishing their Offers or Demands.
Yagna Market is [matching incoming Demands and Offers](
https://docs.google.com/document/d/1yTupuRsN9DKVrK1TPhM6dBxKCAPk0wCB8KxRf57ZkV4
) and creates Proposals. Proposal is a pair of Offer and Demand which are
matching. The matching can be [strong or weak](
https://docs.google.com/document/d/1tzMrhdBr9wiUXtSn1JO18MmIiP31dkMakdjStnF3eZY/edit#heading=h.jzr5wr9i4uh5
). Each Proposal is then fed to the Requestor (ie an issuer of its Demand
component).


### Negotiation Phase
Upon Proposal reception a party (usually the Requestor) can start interaction
with selected counterparty to negotiate the Proposal. During the negotiation
parties are alternately exchanging Proposals with adjusted properties or/and
constraints for owned component to strongly match Offer with Demand.

Current Market implementation does **not** support [dynamic property resolution nor
pseudo-function support](
https://docs.google.com/document/d/1Zny_vfgWV-hcsKS7P-Kdr3Fb0dwfl-6T_cYKVQ9mkNg/edit#heading=h.6y5qk7bcl9qy
) during the Negotiation phase.

### Agreement Phase
The negotiation is successful when the Requestor receives a Proposal with an
Offer satisfying all constrains from his Demand (strong match).
The Requestor can promote such a Proposal into an Agreement. The Agreement
is send to Provider to be finally accepted.

Provider acceptance finishes the Market interaction for both parties and
enables Requestor to start an Activity.


## Decentralized market test suite
To invoke market test suite use:
```
cargo test --workspace --features ya-market/test-suite
```
or for market crate only
```
cargo test -p ya-market --features ya-market/test-suite
```

Note that market test suite uses single thread.

### Running with logs enabled

It is very useful to see logs, if we want to debug test. We can do this as
always by adding RUST_LOG environment variable, but in test case we need to
add `env_logger::init();` on the beginning. 

```
RUST_LOG=debug cargo test -p ya-market --features ya-market/test-suite 
```