# Decentralized Marketplace (market)

The Decentralized Marketplace component in Yagna is responsible for facilitating transactions between Requestors and Providers. It manages the matching of compute offers and demands, handles negotiations, and oversees the creation and management of agreements.

## Key Features

1. **Offer and Demand Matching**: Efficiently matches Provider offers with Requestor demands based on various criteria.
2. **Negotiation Protocol**: Implements a flexible negotiation protocol allowing Requestors and Providers to reach mutually acceptable terms.
3. **Agreement Management**: Handles the creation, confirmation, and termination of agreements between parties.
4. **Event-based Communication**: Utilizes an event system to communicate market activities between participants.
5. **Decentralized Architecture**: Operates in a decentralized manner, without relying on a central authority for matchmaking.

## Marketplace Components

### Matcher

The Matcher is responsible for identifying compatible offers and demands:

1. **Subscription Management**: Handles subscriptions to offer and demand broadcasts.
2. **Matching Algorithm**: Implements efficient algorithms to match offers and demands based on specified criteria.
3. **Re-matching**: Periodically re-evaluates matches to account for changes in the marketplace.

### Negotiator

The Negotiator handles the negotiation process between Requestors and Providers:

1. **Proposal Handling**: Processes incoming proposals and generates counter-proposals.
2. **Negotiation Strategies**: Implements various negotiation strategies to reach optimal agreements.
3. **Agreement Formation**: Finalizes negotiations by forming agreements when terms are mutually accepted.

### Agreement Store

Manages the lifecycle of agreements:

1. **Agreement Storage**: Persists agreement details in a database.
2. **State Management**: Tracks and updates the state of agreements (e.g., proposed, confirmed, terminated).
3. **Retrieval and Querying**: Provides interfaces for retrieving and querying agreement information.

## Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Requestor" as REQ
RECTANGLE "Provider" as PROV
RECTANGLE "Decentralized Marketplace" as MKT {
  RECTANGLE "Matcher" as MATCH
  RECTANGLE "Negotiator" as NEG
  RECTANGLE "Agreement Store" as AGR
}
RECTANGLE "Payment System" as PAY
RECTANGLE "Activity Management" as ACT

REQ --> MKT : Submits demand
PROV --> MKT : Submits offer
MATCH --> NEG : Passes potential matches
NEG --> AGR : Creates agreements
MKT --> PAY : Initiates payments
MKT --> ACT : Triggers activities

@enduml
\```

## Market Protocols

Yagna supports multiple market protocols:

1. **Mk1 (Legacy)**: A broadcast-based protocol where offers and demands are widely disseminated.
2. **Mk2**: A more efficient, decentralized protocol that reduces network overhead and improves scalability.

## Integration with Other Components

The Marketplace interacts with several other Yagna components:

1. **Identity Management**: Verifies the identities of Requestors and Providers participating in the market.
2. **Payment**: Interfaces with the payment system to handle financial transactions related to agreements.
3. **Activity**: Triggers the creation of compute activities once agreements are confirmed.

## Code Example: Creating a Demand

Here's a simplified example of how a Requestor might create a demand in the marketplace:

\```rust
use ya_market::{MarketApi, Demand, DemandBuilder};

async fn create_demand(market_api: &dyn MarketApi) -> Result<String, Box<dyn std::error::Error>> {
    let demand = DemandBuilder::new()
        .set_property("golem.node.id.name", "MyRequestor")
        .set_property("golem.usage.cpu_sec", 3600)
        .set_property("golem.usage.storage_gib", 10)
        .build();

    let subscription_id = market_api.subscribe_demand(&demand).await?;
    Ok(subscription_id)
}

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let market_api = // Initialize MarketApi
    let subscription_id = create_demand(&market_api).await?;
    println!("Created demand subscription: {}", subscription_id);
    Ok(())
}
\```

This example demonstrates:
1. Using the `DemandBuilder` to create a new demand with specific properties.
2. Subscribing the demand to the marketplace using the `MarketApi`.
3. Receiving a subscription ID for further management of the demand.

The Marketplace component ensures efficient matching of compute resources, facilitates negotiations, and manages agreements, forming the backbone of the decentralized compute ecosystem in Yagna.