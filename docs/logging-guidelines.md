# Lightweight Golem Logging Guidelines

## Objective

## Scope

## Logging library requirements

Must have:
- Redirect log stream to **console** (STDOUT/STDERR) and/or **files**
- Filter log entries by log level
- Filter log entries by entry subsets (like namespace subtrees)

Nice to have:
- 

## Log entry content

The log entries should record following aspects/attributes:

Must have:
- **Timestamp** (with ms granularity, in UTC or with TZ information) - must be possible to correlate entries from nodes in different timezones
- Log **level**
- Log **area/topic** (eg. namespace or module of code which records the log entry)
- **Grouping or correlating** attribute (attribute which allows to filter events related to a single command, activity, API request, etc.)
  - Thread id
  - Bespoke correlation id
- Log entry **description** (human readable, no linebreaks)

Nice to have/where applicable:
- Log entry **context information**, variables, parameters
- Low level technical details (eg. network traffic content, API message bodies)
- **Error code**
- **Error resolution hints** - very important for high level errors / warns that actually can be resolved by some action. In some cases this is true also for low level errors / warns
- **Code location** of the log statement


### Data confidentiality

Care must be taken when confidential or personal data need to be recorded in logs:
- When sensitive data must be logged for dignostic purposes, obfuscate, eg. output only leading and/or trailing characters of a sensitive string instead of the full value.

## Log level guidelines

### CRITICAL/FATAL
Purpose: 
- Indicate the app is unable to continue, it should exit after this message.

Audience:
- Users

Examples:

### ERROR
Purpose: 
- Indicate that app is unable to perform the requested action and stops trying.

Audience:
- Users
- Integrator developers
- Core system developers

Examples:

### WARN
Purpose:
- Indicate that app is gracefully handling an erratic situation and is able to continue with the requested action.

Audience:
- Users
- Node owners/admins
- Integrator developers
- Core system developers

Examples:

### INFO
Purpose:
- Indicate that app is performing a requested action/command/request.

Audience:
- Users
- Node owners/admins
- Integrator developers
- Core system developers

Examples:
- Command requested from the module
  - Indication of a CLI command sent to Yagna daemon
  - Indication of REST API request received
  
### DEBUG
Purpose:
- Provide additional context of performed actions, additional steps or any info which may be useful for troubleshooting.

Audience:
- Integrator developers
- Core system developers
- Node owners/admins 

Examples:

### TRACE
Purpose:
- Record low-level/technical details of performed actions, like net traffic content (API requests, responses, Golem Net messages, etc.)

Audience:
- Core system developers

Examples:
- Environment variable/config parameter value snapshots
- API request/response body content
- Golem net message routing info and content


## Error handling & logging guidelines

TODO