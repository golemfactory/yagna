# Agent Applications Overview

Agent applications in Yagna are responsible for managing the interactions between users (Providers and Requestors) and the Yagna network. They handle resource management, task execution, and payment processes.

## Key Components

1. [Provider Agent (ya-provider)](provider.md)
2. [Requestor Agent (ya-requestor)](requestor.md)

## Provider Agent

The Provider Agent manages the resources offered by a Provider node on the Yagna network. It handles:

- Resource advertisement
- Offer creation and management
- Task acceptance and execution
- Payment collection

For more details, see the [Provider Agent documentation](provider.md).

## Requestor Agent

The Requestor Agent facilitates the process of requesting computational resources and executing tasks on the Yagna network. It manages:

- Demand creation
- Provider selection
- Task deployment and monitoring
- Payment disbursement

For more information, refer to the [Requestor Agent documentation](requestor.md).

## Interaction with Core Services

Both Provider and Requestor Agents interact with Yagna's core services, including:

- Market service for offer/demand matching
- Activity service for task execution management
- Payment service for handling transactions
- Identity service for authentication and authorization

Understanding these agent applications is crucial for effectively utilizing the Yagna network, whether you're offering computational resources or seeking to execute tasks.