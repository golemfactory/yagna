# Lightweight Golem Logging Guidelines

## Objective

The purpose of this article is to provide a set of prescriptive guidelines for developers to follow in order to achieve consistent content of execution logs.
Execution logs serve two main purposes, depending on "audience":
- **Core system developers** - Efficient troubleshooting and debugging during development of Golem modules.
- **Integrator developers** - Troubleshooting and debugging of applications which use Golem as platform.
- **Users** - Diagnostics of issues appearing on owned Golem nodes (eg. setup, infrastructural or maintenance-related issues).

Ideal logs contain the right amount of information for the situation and audience. This implies that "too much logs" may be as useless as "no logs" - therefore focus is put on recording appropriate information at each log level.

The article includes also **generic requirements** referring to the **logging framework** chosen by developers for specific platform they are working on - this takes into account that Golem is a multi-platform software ecosystem, and however many programming languages can (in theory) be used to develop Golem components, their developers should maintain consistent approach to logs & audit trails, to provide uniform level of "user & developer experience".

## Scope

These guidelines adhere to software published by Golem Factory as parts of Lightweight Golem/Yagna ecosystem. This includes among others, the following repos:
- [yagna](https://github.com/golemfactory/yagna)
- [ya-client](https://github.com/golemfactory/ya-client)
- [yapapi](https://github.com/golemfactory/yapapi)
- [yagna-integration](https://github.com/golemfactory/yagna-integration)

## Logging library requirements

Must have:
- Redirect log stream to **console** (STDOUT/STDERR) and/or **files**
- Filter log entries by log level
- Filter log entries by entry subsets (like namespace subtrees)

Nice to have:
- ...

## Log entry content

The log entries should record following aspects/attributes:

**Must have:**
- **Timestamp** (with ms granularity, in UTC or with TZ information) - must be possible to correlate entries from nodes in different timezones
- Log **level**
- Log **area/topic** (eg. namespace or module of code which records the log entry)
- **Grouping or correlating** attribute (attribute which allows to filter events related to a single command, activity, API request, etc.)
  - Thread id
  - Bespoke correlation id
- Log entry **description** (human readable, no linebreaks)

**Nice to have/where applicable:**

- Log entry **context information**, variables, parameters
- Low level technical details (eg. network traffic content, API message bodies)
- **Error code**
- **Error resolution hints** - very important for high level errors / warns that actually can be resolved by some action. In some cases this is true also for low level errors / warns
- **Code location** of the log statement

### Generic guidelines
- Use descriptive messages and proper casing/punctuation, ie. instead of: 
  ```
  [2020-08-27T07:56:22Z DEBUG yagna] init gnt drv
  ```
  do this:
  ```
  [2020-08-27T07:56:22Z DEBUG yagna] Initializing GNT payment driver...
  ```

### Data confidentiality

Care must be taken when confidential or personal data need to be recorded in logs:
- When sensitive data must be logged for dignostic purposes, obfuscate, eg. output only leading and/or trailing characters of a sensitive string instead of the full value.

## Log level guidelines

### CRITICAL/FATAL
**Purpose:** 
- Indicate the app is unable to continue, it should exit after this message.

**Audience:**
- Users

**Examples:**
- Uncaught exceptions/unhandled errors

### ERROR
**Purpose:** 
- Indicate that app is unable to perform the requested action and stops trying.

**Audience:**
- Users
- Integrator developers
- Core system developers

**Examples:**

### WARN
**Purpose:**
- Indicate that app is gracefully handling an erratic situation and is able to continue with the requested action.

**Audience:**
- Users
- Node owners/admins
- Integrator developers
- Core system developers

**Examples:**

### INFO
**Purpose:**
- Inform about successful initialization and shutdown of app module/feature.
- Indicate that app is performing a requested action/command/request.

**Audience:**
- Users
- Node owners/admins
- Integrator developers
- Core system developers

**Examples:**
- Startup event of a significant module - including fundamental parameters of execution, eg. 
  - URLs/port numbers for services listening or depending on network connectivity,
  - Working directories, data directories 
- Shutdown event of a significant module 
- Command requested from the module
  - Indication of a CLI command sent to Yagna daemon
  - Indication of REST API request received
  
### DEBUG
**Purpose:**
- Provide additional context of performed actions, additional steps or any info which may be useful for troubleshooting.

**Audience:**
- Integrator developers
- Core system developers
- Node owners/admins 

**Examples:**
- Low level processing steps, especially those dependent on files, resources, connectivity, with dependency info (URLs, addresses, parameter values)
- 

### TRACE
**Purpose:**
- Record low-level/technical details of performed actions, like net traffic content (API requests, responses, Golem Net messages, etc.)

**Audience:**
- Core system developers

**Examples:**
- Environment variable/config parameter value snapshots
- API request/response body content
- Golem net message routing info and content

## Error handling & logging guidelines

As low-level, 3rd party library errors are encountered during execution, their error messages are usually useless without context information. It is vital to wrap the low-level error messages with additional info to indicate details of performed activity that would aid in troubleshooting. Eg. instead of:

```
[2020-08-27T07:56:22Z ERROR yagna] File IO error: Path not found
```

log this:

```
[2020-08-27T07:56:22Z ERROR yagna] E00342 - WASM ExeUnit DEPLOY: Error downloading remote file to temp folder '/tmp/yagna_data': File IO error: Path not found

```
