# Activity Management (activity)

The Activity Management component in Yagna is responsible for controlling the execution of tasks on Provider nodes. It manages the lifecycle of activities, interacts with ExeUnits, and ensures proper execution and monitoring of compute tasks.

## Key Features

1. **Activity Lifecycle Management**: Handles the creation, execution, and termination of activities.
2. **ExeUnit Interaction**: Communicates with ExeUnits to deploy and run tasks in isolated environments.
3. **State Management**: Tracks and updates the state of activities throughout their lifecycle.
4. **Resource Allocation**: Manages the allocation of compute resources for activities.
5. **Result Handling**: Collects and processes the results of completed activities.

## Activity Lifecycle

1. **Creation**: An activity is created when a Requestor initiates a task based on an agreement.
2. **Deployment**: The activity is deployed to an appropriate ExeUnit on the Provider node.
3. **Execution**: The ExeUnit runs the activity, executing the specified computations.
4. **Monitoring**: The activity's progress and resource usage are monitored throughout execution.
5. **Completion/Termination**: The activity is completed when the task finishes or terminated if issues arise.

## Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Marketplace" as MKT
RECTANGLE "Activity Management" as ACT {
  RECTANGLE "Lifecycle Manager" as LM
  RECTANGLE "State Manager" as SM
  RECTANGLE "Resource Manager" as RM
}
RECTANGLE "ExeUnit" as EU
RECTANGLE "Payment System" as PAY

MKT --> ACT : Initiates activities
ACT --> EU : Deploys and controls tasks
ACT --> PAY : Reports billable usage
LM --> ACT : Manages activity lifecycle
SM --> ACT : Tracks activity states
RM --> ACT : Allocates resources

@enduml
\```

## Integration with Other Components

The Activity Management component interacts with several other Yagna components:

1. **Marketplace**: Receives activity requests based on confirmed agreements.
2. **Payment**: Triggers payments based on activity execution and resource usage.
3. **Identity Management**: Verifies the identities of Requestors and Providers involved in activities.
4. **ExeUnit**: Manages the actual execution of tasks in isolated environments.

## Code Example: Creating and Managing an Activity

Here's a simplified example of how an activity might be created and managed:

\```rust
use ya_activity::{ActivityApi, ActivityDescription, ActivityState};

async fn create_and_run_activity(
    activity_api: &dyn ActivityApi,
    agreement_id: &str,
    task_package: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let description = ActivityDescription {
        agreement_id: agreement_id.to_string(),
        task_package,
        // ... other necessary details
    };

    let activity_id = activity_api.create_activity(description).await?;
    
    activity_api.deploy_activity(&activity_id).await?;
    activity_api.start_activity(&activity_id).await?;

    // Wait for activity to complete
    loop {
        let state = activity_api.get_activity_state(&activity_id).await?;
        match state {
            ActivityState::Completed => break,
            ActivityState::Terminated => return Err("Activity terminated unexpectedly".into()),
            _ => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
        }
    }

    let result = activity_api.get_activity_result(&activity_id).await?;
    println!("Activity completed with result: {:?}", result);

    Ok(())
}

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let activity_api = // Initialize ActivityApi
    create_and_run_activity(&activity_api, "agreement_123", vec![/* task package */]).await?;
    Ok(())
}
\```

This example demonstrates:
1. Creating an activity with a specific agreement ID and task package.
2. Deploying and starting the activity.
3. Monitoring the activity state until completion.
4. Retrieving the activity result.

The Activity Management component ensures proper execution and control of compute tasks, integrating closely with ExeUnits and other Yagna components to provide a robust and flexible compute environment.