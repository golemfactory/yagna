# Provider overview

## Installation

**Default installation:**

`curl -sSf https://join.golem.network/as-provider | bash -`

**Installing specific version:**

`curl -sSf https://join.golem.network/as-provider | YA_INSTALLER_CORE=pre-rel-v0.13.0-rc10 bash -`

`YA_INSTALLER_CORE` should point to tag in yagna repository.

To change default runtimes versions set env variables:

- `YA_INSTALLER_WASI=v0.2.2`
- `YA_INSTALLER_VM=v0.3.0`

## Provider directories

Standard Provider installation uses files in following directories:


| name                    | ENV           | command line                | default configuration                     | description                                                                                                                                                                                                            | comment                                                                                                                                                                                 |
| ----------------------- | ------------- | --------------------------- | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Yagna data directory    | YAGNA_DATADIR | yagna --datadir             | ~/.local/share/yagna                      | Contains yagna daemon configuration and persistent files.                                                                                                                                                              |                                                                                                                                                                                         |
| Provider data directory | DATADIR       | ya-provider --datadir       | ~/.local/share/ya-provider                | Provider agent configuration files, logs and ExeUnit directories.                                                                                                                                                      |                                                                                                                                                                                         |
| Runtimes directory      | EXE_UNIT_PATH | ya-provider --exe-unit-path | ~/.local/lib/yagna/plugins/ya-*.json      | Contains runtime binaries.                                                                                                                                                                                             | Regular expression pointing to ExeUnits descriptors (It's not directory). Warning:`golemsp` overrides this setting (issue: [#2689](https://github.com/golemfactory/yagna/issues/2689)). |
| ExeUnit cache           |               |                             | ~/.local/share/ya-provider/exe-unit/cache | Stores cached ExeUnit Runtime images.                                                                                                                                                                                  | Always relative to Provider data directory `${YAGNA_DATADIR}/exe-unit/cache`.                                                                                                           |
| ExeUnit working dir     |               |                             | ~/.local/share/ya-provider/exe-unit/work  | Directory used to store tasks data. For each Agreement ExeUnit creates directory named by Agreement Id. Inside there are directories created for each activity. VM runtime mounts image volumes inside this directory. | Always relative to Provider data directory `${YAGNA_DATADIR}/exe-unit/work`.                                                                                                            |
| Binaries                |               |                             | ~/.local/bin/yagna                        | Yagna daemon and agent binaries.                                                                                                                                                                                       | If yagna is already installed, installer will use previous directory instead.                                                                                                           |
| Installer files         |               |                             | ~/.local/share/ya-installer               | Directory used by installer to download files.                                                                                                                                                                         | Files can be removed after installation is completed.                                                                                                                                   |
| GSB unix socket         | GSB_URL       |                             | unix:/tmp/yagna.sock                      | Unix socket used by GSB for communication.                                                                                                                                                                             | Can be configured to use TCP.                                                                                                                                                           |
