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

## ExeUnit Integration

ExeUnits are responsible for running user code within isolated environments:

1. **Runtime Selection**: Chooses appropriate runtime (e.g., WASM, Docker) based on activity requirements.
2. **Deployment**: Prepares the runtime environment and deploys the activity code.
3. **Execution Control**: Starts, pauses, resumes, and stops activity execution.
4. **Resource Management**: Enforces resource limits and tracks usage.

## State Management

Activities can be in various states throughout their lifecycle:

1. **New**: Activity has been created but not yet deployed.
2. **Deploying**: Activity is being prepared for execution.
3. **Ready**: Activity is deployed and ready to start.
4. **Running**: Activity is currently executing.
5. **Suspended**: Activity execution has been temporarily paused.
6. **Completed**: Activity has finished execution successfully.
7. **Terminated**: Activity has been stopped due to an error or external request.

## Integration with Other Components

The Activity Management component interacts with several other Yagna components:

1. **Marketplace**: Receives activity requests based on confirmed agreements.
2. **Payment**: Triggers payments based on activity execution and resource usage.
3. **Identity Management**: Verifies the identities of Requestors and Providers involved in activities.

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