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
