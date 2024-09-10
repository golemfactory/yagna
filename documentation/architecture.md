# Yagna Architecture Overview

Yagna is a decentralized platform that enables distributed computing and resource sharing. This document provides a high-level overview of the Yagna architecture, explaining its key components and their interactions.

## Core Components

Yagna consists of several core components that work together to create a robust and flexible ecosystem:

1. Network Communication (net)
2. Decentralized Marketplace (market)
3. Payment System (payment)
4. Activity Management (activity)
5. Identity Management (identity)
6. Application Manifest Management (manifest)
7. Cryptography (crypto)
8. Access Control Lists (acl)
9. Golem File Transfer Protocol (gftp)
10. Service Bus API (gsb-api)
11. Metrics Service (metrics)
12. Internal Data Model (model)

## High-Level Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Requestor Agent" as RA
RECTANGLE "Provider Agent" as PA
RECTANGLE "Yagna Daemon" as YD {
RECTANGLE "Network (net)" as NET
RECTANGLE "Marketplace (market)" as MKT
RECTANGLE "Payment (payment)" as PAY
RECTANGLE "Activity (activity)" as ACT
RECTANGLE "Identity (identity)" as ID
RECTANGLE "Manifest (manifest)" as MAN
RECTANGLE "Crypto (crypto)" as CRY
RECTANGLE "ACL (acl)" as ACL
RECTANGLE "GFTP (gftp)" as GFTP
RECTANGLE "GSB API (gsb-api)" as GSB
RECTANGLE "Metrics (metrics)" as MET
RECTANGLE "Model (model)" as MOD
}
RECTANGLE "ExeUnit" as EU

RA --> YD : Interacts with
PA --> YD : Interacts with
YD --> EU : Manages
NET <--> MKT : Facilitates communication
MKT <--> PAY : Handles transactions
ACT --> EU : Controls
ID --> ACL : Authenticates
MAN --> ACT : Defines tasks
CRY --> NET : Secures communication
GFTP --> NET : Transfers files
GSB --> NET : Provides service bus
MET --> YD : Collects metrics
MOD --> YD : Defines data structures

@enduml
\```

## Component Interactions

1. **Requestor and Provider Agents** interact with the Yagna Daemon to participate in the network, create demands/offers, and manage tasks.

2. The **Network (net)** component facilitates communication between nodes, using both decentralized (Hybrid Net) and centralized (Central Net) approaches.

3. The **Marketplace (market)** matches offers and demands, facilitating negotiations between Requestors and Providers.

4. The **Payment (payment)** system handles transactions, using various payment drivers (e.g., ERC20, Dummy) to process payments and manage allocations.

5. **Activity (activity)** management controls task execution, interacting with ExeUnits to run computations in isolated environments.

6. **Identity (identity)** manages user authentication and authorization, working with the ACL component to enforce access controls.

7. The **Manifest (manifest)** component processes and validates application manifests, defining task requirements and permissions.

8. **Cryptography (crypto)** provides security functions used throughout the system, especially in network communications.

9. **GFTP (Golem File Transfer Protocol)** handles efficient file transfers between nodes.

10. The **GSB API (Service Bus API)** provides a unified interface for inter-service communication within Yagna.

11. The **Metrics (metrics)** service collects and exposes performance data from various components.

12. The **Model (model)** component defines the internal data structures used across Yagna components.
    This architecture allows Yagna to provide a flexible, secure, and efficient platform for distributed computing, enabling complex workflows and resource sharing across a decentralized network of nodes.
