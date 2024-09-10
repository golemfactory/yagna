# Service Bus API (gsb-api)

The Service Bus API (gsb-api) in Yagna provides a unified interface for inter-service communication within the platform. It facilitates message routing, service discovery, and seamless interaction between various components of the Yagna ecosystem.

## Key Features

1. **Service Registration**: Allows services to register themselves with the GSB.
2. **Message Routing**: Efficiently routes messages between services based on destination addresses.
3. **Service Discovery**: Enables services to discover and communicate with each other dynamically.
4. **Protocol Abstraction**: Abstracts away the underlying transport protocols, providing a consistent API.
5. **Asynchronous Communication**: Supports both synchronous and asynchronous communication patterns.

## GSB Architecture

The GSB consists of several key components:

1. **Router**: Manages message routing between services.
2. **Service Registry**: Maintains a registry of available services and their addresses.
3. **Transport Layer**: Handles the actual transmission of messages using various protocols.
4. **Serialization/Deserialization**: Converts messages between their in-memory and wire formats.

## Message Types

The GSB supports various types of messages:

1. **Request**: A message that expects a response.
2. **Response**: A reply to a request message.
3. **Event**: A one-way message that doesn't expect a response.

## Integration with Other Components

The GSB-API interacts with virtually all other Yagna components, serving as the primary means of inter-service communication:

1. **Network (net)**: Utilizes the network layer for message transmission.
2. **Cryptography (crypto)**: Ensures secure message exchange.
3. **Identity Management**: Verifies the identity of services during registration and communication.
4. **All Core Services**: Provides communication capabilities to market, payment, activity, and other core services.

## Code Example: Implementing a GSB Service

Here's a simplified example of how a service might be implemented using the GSB-API:

\```rust
use ya_service_bus::{typed as bus, RpcMessage, RpcEndpoint};

#[derive(RpcMessage)]
#[rpc(protocol = "my-service")]
struct MyRequest {
    data: String,
}

#[derive(RpcMessage)]
#[rpc(protocol = "my-service")]
struct MyResponse {
    result: String,
}

struct MyService;

impl RpcEndpoint for MyService {
    type Request = MyRequest;
    type Response = MyResponse;

    async fn handle(&self, request: MyRequest) -> Result<MyResponse, ()> {
        let result = format!("Processed: {}", request.data);
        Ok(MyResponse { result })
    }
}

async fn run_service() -> Result<(), Box<dyn std::error::Error>> {
    let service = MyService;
    bus::bind_service(&service).await?;
    
    // Keep the service running
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn call_service() -> Result<(), Box<dyn std::error::Error>> {
    let request = MyRequest {
        data: "Hello, GSB!".to_string(),
    };
    
    let response: MyResponse = bus::service("my-service")
        .send(request)
        .await?;
    
    println!("Received response: {}", response.result);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::spawn(run_service());
    call_service().await?;
    Ok(())
}
\```

This example demonstrates:
1. Defining request and response message types.
2. Implementing a service that handles requests.
3. Binding the service to the GSB.
4. Calling the service from another part of the application.

The GSB-API provides a powerful and flexible way for services within Yagna to communicate, enabling complex interactions while abstracting away the details of network communication and service discovery.