This page describes the logging mechanics and lists guidelines for clear logging.

## Goal
There are 2 main goals of the logging: providing support and tracing down errors in development

Logging with the support goal the is to inform semi-technical users about the state of the application.
This means all levels from INFO and up need to be clear to understand for everyone.

Logging with the development goal is to find where errors originate from. This means every log message needs sufficient information to trace the application its steps.

Use cases for the logs are:
- Support and development teams will receive an end-user log to investigate
- Run the application in DEBUG log-level to see what goes on in detail.

## Levels
These are the log levels available to the logger:
- **CRITICAL** = app is unable to continue, it should exit after this message
- **ERROR** = app is unable to perform the requested action and stops trying
- **WARNING** = app is unable to perform the requested action but will continue to run, for instance try again later
- **INFO** = app performs a requested action
- **DEBUG** = gives extra details over other messages, always a following message

## Logger initialization
A logger can be created at the top of the file, after all imports are completed like this:

```
...
import logging

logger = logging.getLogger(__name__)
```
- `logger` has to be the module level variable name.
- `__name__` has to be the logger name
- when there is a clear and good reason, `__name__` can be be replaced with:
    - base level script like `golemapp` and `golemcli`
    - 3rd part module name like `twisted`
    - higher level module, like `golem.a.b` for `golem.a.b.c`
    - help your future self and comment the reason

## Log message structure
Log messages should start with a message that describes the action clearly. At the end there should be extra information, for example the operation, status or task_id.
```
logger.info('Task has been started. id=%r', task_id)
logger.debug('task_header=%r', task_header)
```
Debug messages are used to enrich the messages logged in higher levels.

Some guidelines on what to ( and what not to ) log:
- Describe the action as short and clear as possible
- Always start with a message, print data as "`kwargs`"
    - For example `Task deadline passed. id=%r, state=%r`
- Always print enough related data for the action you log
- Always use native logger formatting, no manual formatting.
    - When a log-level is disabled there is no need to format all these strings.
    - [PoC](https://gist.github.com/jiivan/eedb505574ccb2503af48fa2e3043036)
- "Silly" logs (multiline data, spam, json) should always be DEBUG
- Info all important milestones
    - Listening ports
    - Components load
    - User interactions
    - Task interactions
    - Network interactions
- Include node names in messages, before node_id's
- Prefer to use `abrev` node_id ( `node_id[:8] + ‘…’ + node_id[-8:]` ) over the full id when you know there is sufficient data
    - First contact ( subtask given, connected ) should print full id
    - Repeating messages should print the abrev version (either `golem.core.common.node_info_str` or `golem.core.common.short_node_id`)
    - Increase basic abrev function when network grows and 8 byte is not unique enough
- No JSON ( except DEBUG )
- No Stack/Traceback ( on INFO or WARNING)

## Defaults
By default the golem logger has 3 outputs:
- console -> level INFO
- `golem.log` -> level INFO, rotate daily, keep 5
- `golem.error.log` -> level WARNING, no rotate

External components are by default set to the WARNING level, since INFO logs are not always useful for our end users.

## Arguments
`--log-level` is the argument to tweak the logs written by the application. When set it will change the INFO console and file handlers and all golem components. It shouldn't affect `golem.error.log`.

`--loglevel` is an argument used by our 3rd party library crossbar, since this is a 3rd party log it only shows WARNING and up inside golem application logs.
Please note: Crossbar log level's are lower case, while golem log level's are upper case. Also `warn` in crossbar is equal to `WARNING` in golem. The rest of the levels are named the same.