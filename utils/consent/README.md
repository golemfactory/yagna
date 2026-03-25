## Feature Documentation

### Aim:
Add a management feature to allow users to set their consent for data collection and publishing on the stats.golem.network.

### Description:
The user setting for the consent is saved in the CONSENT file, in the YAGNA_DATADIR folder.
Both ```yagna``` and ```golemsp``` use the config (see details below).
The setting can be modified by using the YA_CONSENT_STATS env variable (that can be read from the .env file).

### Used artefacts:
YA_CONSENT_STATS - env, the value set by the variable has priority and is used to update the setting in the CONSENT file when yagna or golemsp is run
CONSENT file in the YAGNA_DATADIR folder

### How to check the settings:

Shows the current setting,
```
yagna consent show
```
Note it reads the value from the CONSENT file and the value of the YA_CONSENT_STATS variable (from session or .env file in the pwd folder) so if the service was launched from another folder or with a different value of YA_CONSENT_STATS set in the session the information shown setting may be not accurate.

### How to change the settings:

set the new setting in the CONSENT file, requires yagna restart to take effect.
- yagna consent allow/deny <consent_scope>
- restart yagna/golemsp with YA_CONSENT_STATS set, the setting in the CONSENT file will be updated to the value set by the variable.

### Details:

```golemsp``` will ask the question about the consent if it cannot be determined from the YA_CONSENT_STATS variable or CONSENT file.
If Yagna cannot determine the settings from the YA_CONSENT_STATS variable or CONSENT file it will assume the consent is not given, but will not set it in the CONSENT file.

### Motivation:
```golemsp``` is designed to install the provider nodes interactively. Therefore, it will expect the question to be answered. The user still can avoid the question by setting the env variable.
The default answer is "allow" as we do not collect data that is both personal and not already publicly available for the other network users. The data is used to augment the information shown on the stats.golem.network and most of the providers expect these data to be available there.
Yagna on the other hand won't stop on the question if the setting is not defined, to prevent the interruption of automatic updates of Yagna that run as a background service.
We expect such a scenario mostly for requestors.