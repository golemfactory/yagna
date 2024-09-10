# Core Components Overview

This section provides an overview of the core components that make up the Yagna platform. Each component plays a crucial role in enabling the decentralized computation and resource sharing capabilities of Yagna.

## Components

1. [Network Communication (net)](net.md)

    - Handles communication between nodes in the Yagna network.
    - Implements Hybrid Net and Central Net strategies.

2. [Decentralized Marketplace (market)](market.md)

    - Facilitates the matching of compute offers and demands.
    - Manages negotiations and agreements between Requestors and Providers.

3. [Payment System (payment)](payment.md)

    - Processes transactions and manages payment allocations.
    - Supports multiple payment drivers, including ERC20 and Dummy drivers.

4. [Activity Management (activity)](activity.md)

    - Controls the execution of tasks on Provider nodes.
    - Manages the lifecycle of activities and interacts with ExeUnits.

5. [Identity Management (identity)](identity.md)

    - Handles user identities and authentication within the Yagna ecosystem.
    - Manages AppKeys for secure access to Yagna services.

6. [Application Manifest Management (manifest)](manifest.md)

    - Processes and validates application manifests.
    - Defines task requirements, capabilities, and permissions.

7. [Cryptography (crypto)](crypto.md)

    - Provides cryptographic functions for secure communication and data handling.
    - Manages key generation, signing, and encryption operations.

8. [Access Control Lists (acl)](acl.md)

    - Implements authorization mechanisms for Yagna services.
    - Defines and enforces access permissions for different identities.

9. [Golem File Transfer Protocol (gftp)](gftp.md)

    - Enables efficient file transfers between nodes in the Yagna network.
    - Implements a custom protocol optimized for distributed environments.

10. [Service Bus API (gsb-api)](gsb-api.md)

    - Provides an API for inter-service communication within Yagna.
    - Facilitates message routing and service discovery.

11. [Metrics Service (metrics)](metrics.md)

    - Collects and exposes performance metrics from various Yagna components.
    - Enables monitoring and optimization of the Yagna platform.

12. [Internal Data Model (model)](model.md)
    - Defines the core data structures used across Yagna components.
    - Ensures consistency in data representation throughout the system.

Each of these components is essential to the functioning of Yagna as a whole. They work together to create a robust, secure, and efficient platform for decentralized computation and resource sharing.
