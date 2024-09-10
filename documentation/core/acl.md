# Access Control Lists (ACL)

The Access Control Lists (ACL) component in Yagna is responsible for implementing authorization mechanisms and enforcing access permissions for different identities within the Yagna ecosystem. It works closely with the Identity Management component to ensure secure and controlled access to Yagna services and resources.

## Key Features

1. **Fine-grained Access Control**: Defines and enforces detailed permissions for various actions and resources in Yagna.
2. **Flexible Permission Models**: Supports role-based, identity-based, and resource-based access control models.
3. **Integration with Identity Management**: Works in tandem with the Identity Management component to authenticate and authorize users and services.
4. **Dynamic Updates**: Allows for real-time updates to access permissions without system restarts.

## ACL Structure

The ACL in Yagna typically consists of the following elements:

1. **Subject**: The identity (user, service, or node) to which the permission applies.
2. **Resource**: The Yagna service, API endpoint, or data object being accessed.
3. **Action**: The operation being performed (e.g., read, write, execute).
4. **Permission**: The level of access granted (e.g., allow, deny).

## Permission Types

Yagna's ACL supports various types of permissions:

1. **Global Permissions**: Apply to all resources of a certain type.
2. **Resource-specific Permissions**: Apply to individual resources or resource instances.
3. **Role-based Permissions**: Assign permissions based on predefined roles.
4. **Temporary Permissions**: Time-limited access rights for specific operations.

## Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Identity Management" as IDM
RECTANGLE "Access Control Lists (ACL)" as ACL {
  RECTANGLE "Permission Manager" as PM
  RECTANGLE "Role Manager" as RM
  RECTANGLE "Resource Manager" as ResM
}
RECTANGLE "Yagna Services" as YS
RECTANGLE "API Gateway" as API

IDM --> ACL : Provides identity info
ACL --> YS : Enforces permissions
API --> ACL : Checks permissions
PM --> ACL : Manages permissions
RM --> ACL : Manages roles
ResM --> ACL : Manages resources

@enduml
\```

## Integration with Other Components

The ACL component interacts with several other Yagna components:

1. **Identity Management**: Receives authenticated identity information for authorization decisions.
2. **API Gateway**: Enforces access control for incoming API requests.
3. **Marketplace (market)**: Ensures that only authorized parties can participate in specific market activities.
4. **Activity Management**: Controls access to task execution and management functions.

## Code Example: Checking Permissions

Here's a simplified example of how permissions might be checked using the ACL component:

\```rust
use ya_acl::{AclManager, Permission, Resource, Action};

async fn check_permission(
    acl_manager: &AclManager,
    identity: &str,
    resource: Resource,
    action: Action
) -> Result<bool, Box<dyn std::error::Error>> {
    let permission = acl_manager.check_permission(identity, resource, action).await?;
    Ok(permission == Permission::Allow)
}

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let acl_manager = AclManager::new();
    let identity = "user123";
    let resource = Resource::ApiEndpoint("/market/offers".to_string());
    let action = Action::Read;

    let is_allowed = check_permission(&acl_manager, identity, resource, action).await?;
    println!("Access allowed: {}", is_allowed);
    Ok(())
}
\```

This example demonstrates:
1. Using the `AclManager` to check permissions for a specific identity, resource, and action.
2. Defining resources and actions as structured types for precise access control.
3. Interpreting the permission result to determine if access is allowed.

The ACL component ensures that all access to Yagna resources and services is properly authorized, maintaining the security and integrity of the Yagna ecosystem.