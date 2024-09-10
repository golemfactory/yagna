# Internal Data Model (model)

The Internal Data Model component in Yagna defines the core data structures used across various components of the platform. It ensures consistency in data representation and provides a common language for different parts of the system to communicate and interact.

## Key Features

1. **Standardized Data Structures**: Defines common data types used throughout Yagna.
2. **Serialization Support**: Implements serialization and deserialization for data exchange.
3. **Version Compatibility**: Manages versioning of data structures to ensure backward compatibility.
4. **Type Safety**: Leverages Rust's type system to ensure data integrity and prevent runtime errors.
5. **Cross-Component Consistency**: Ensures that all components use the same data representations.

## Core Data Structures

The model defines several key data structures, including but not limited to:

1. **Node**: Represents a node in the Yagna network.
2. **Activity**: Describes a compute activity.
3. **Agreement**: Represents an agreement between a Requestor and a Provider.
4. **Offer**: Defines a Provider's offer of compute resources.
5. **Demand**: Represents a Requestor's demand for compute resources.
6. **Payment**: Describes a payment transaction.
7. **Identity**: Represents a user or service identity.

## Integration with Other Components

The Internal Data Model is used by virtually all other Yagna components:

1. **Network (net)**: Uses model definitions for network messages.
2. **Marketplace (market)**: Utilizes offer, demand, and agreement models.
3. **Payment**: Uses payment and transaction models.
4. **Activity**: Employs activity and task models.
5. **Identity Management**: Uses identity and credential models.

## Code Example: Defining and Using a Model

Here's a simplified example of how a data model might be defined and used:

\```rust
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agreement {
    pub id: String,
    pub provider_id: String,
    pub requestor_id: String,
    pub creation_date: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    pub state: AgreementState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgreementState {
    Proposal,
    Pending,
    Approved,
    Rejected,
    Terminated,
}

impl Agreement {
    pub fn new(provider_id: String, requestor_id: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            provider_id,
            requestor_id,
            creation_date: now,
            valid_to: now + chrono::Duration::hours(1),
            state: AgreementState::Proposal,
        }
    }

    pub fn approve(&mut self) {
        self.state = AgreementState::Approved;
    }
}

fn main() {
    let mut agreement = Agreement::new(
        "provider123".to_string(),
        "requestor456".to_string(),
    );

    println!("New agreement: {:?}", agreement);

    agreement.approve();
    println!("Approved agreement: {:?}", agreement);

    // Serialize to JSON
    let json = serde_json::to_string(&agreement).unwrap();
    println!("Serialized agreement: {}", json);

    // Deserialize from JSON
    let deserialized: Agreement = serde_json::from_str(&json).unwrap();
    println!("Deserialized agreement: {:?}", deserialized);
}
\```

This example demonstrates:
1. Defining a structured `Agreement` type with associated data.
2. Implementing methods for creating and modifying agreements.
3. Using Serde for serialization and deserialization.
4. Basic usage of the model in a main function.

The Internal Data Model component provides the foundation for data representation across the Yagna platform, ensuring consistency and enabling efficient communication between different components.