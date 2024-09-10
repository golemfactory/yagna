# Network Communication (net)

The network communication component in Yagna is responsible for enabling secure and efficient communication between nodes in the network. It implements various networking strategies to ensure reliable data transfer and node discovery.

## Hybrid Net

Hybrid Net is the primary networking implementation in Yagna, designed to provide a balance between decentralization and efficiency.

### Features

1. **Decentralized Architecture**: Nodes can communicate directly without relying on central servers.
2. **NAT Traversal**: Supports communication between nodes behind NATs using techniques like hole punching.
3. **Encryption**: All communications are encrypted to ensure data privacy and integrity.
4. **Node Discovery**: Implements distributed node discovery mechanisms.

### Transport Mechanisms

Hybrid Net supports multiple transport protocols:

1. **UDP**: For fast, connectionless communication.
2. **TCP**: For reliable, connection-oriented communication.
3. **WebRTC**: For browser-based communication and additional NAT traversal capabilities.

## Central Net (Legacy)

Central Net is a legacy networking implementation that relies on a centralized server (hub) for communication between nodes.

### Features

1. **Centralized Architecture**: All communication passes through a central hub.
2. **Simplified Connectivity**: Easier to establish connections between nodes behind NATs.
3. **Compatibility**: Maintained for backward compatibility with older Yagna implementations.

## GSB (Service Bus)

The GSB (Golem Service Bus) is a crucial component of Yagna's networking infrastructure, providing a flexible and efficient way for services to communicate.

### Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Service A" as SA
RECTANGLE "Service B" as SB
RECTANGLE "GSB" as GSB {
  RECTANGLE "Router" as Router
  RECTANGLE "Transport Layer" as TL
}

SA --> GSB : Sends messages
GSB --> SB : Delivers messages
Router --> TL : Uses

@enduml
\```

### Message Structure

GSB messages consist of the following components:

1. **Destination Address**: Specifies the target service and method.
2. **Payload**: Contains the actual message data.
3. **Reply To**: Address for sending responses (optional).
4. **Request ID**: Unique identifier for request-response correlation.
5. **Message Type**: Indicates whether it's a request, response, or event.

### Message Handling

1. Services register with the GSB, specifying their address and supported methods.
2. When a message is sent, the GSB router determines the appropriate destination based on the address.
3. The router forwards the message to the correct service using the transport layer.
4. If a reply is expected, the sending service can specify a "Reply To" address.

### Transport

GSB supports multiple transport protocols:

1. **IPC (Inter-Process Communication)**:
   - Unix sockets (on Unix-based systems)
   - Named Pipes (on Windows)
2. **TCP**: For network communication between nodes

### Security

GSB implements several security measures:

1. **Authentication**: Services must authenticate before registering or sending messages.
2. **Authorization**: Access control lists (ACLs) determine which services can communicate with each other.
3. **Encryption**: All messages are encrypted in transit.

## Code Example: Sending a Message via GSB

Here's a simplified example of how a service might send a message using the GSB:

\```rust
use ya_service_bus::{typed as bus, RpcMessage};

#[derive(RpcMessage)]
#[rpc(protocol = "proto")]
struct MyMessage {
    content: String,
}

async fn send_message() -> Result<(), Box<dyn std::error::Error>> {
    let msg = MyMessage {
        content: "Hello, GSB!".to_string(),
    };
    
    let response: String = bus::service("destination-service")
        .send(msg)
        .await?;
    
    println!("Received response: {}", response);
    Ok(())
}
\```

This example demonstrates:
1. Defining a message type (`MyMessage`) that implements the `RpcMessage` trait.
2. Using the `bus::service()` function to specify the destination service.
3. Sending the message and awaiting a response.

The GSB handles the routing, serialization, and transport of the message, abstracting away the complexities of network communication from the service developer.
