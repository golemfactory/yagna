# ExeUnits

## How Provider agent finds ExeUnits?

`ya-provider` lists available ExeUnits by listing descriptors placed in [directory](../overview.md#provider-directories). 

Example directory content:
```commandline
nieznanysprawiciel@laptop:~$ tree ~/.local/lib/yagna/plugins
/home/nieznanysprawiciel/.local/lib/yagna/plugins
├── exe-unit
├── ya-runtime-vm
│   ├── runtime
│   │   ├── bios-256k.bin
│   │   ├── efi-virtio.rom
│   │   ├── initramfs.cpio.gz
│   │   ├── kvmvapic.bin
│   │   ├── linuxboot_dma.bin
│   │   ├── self-test.gvmi
│   │   ├── vmlinuz-virt
│   │   └── vmrt
│   └── ya-runtime-vm
├── ya-runtime-vm.json
├── ya-runtime-wasi
└── ya-runtime-wasi.json
```
ExeUnit descriptors must be placed directly in plugins directory (not in nested structure).
Other ExeUnits' files can be moved into subdirectories, because descriptor is pointing to specific files.

## Descriptor

Example ExeUnit descriptor:
```json
[
  {
    "name": "custom",
    "version": "0.2.2",
    "supervisor-path": "exe-unit",
    "runtime-path": "custom-runtime/custom",
    "description": "Custom runtime for documentation purposes.",
    "extra-args": ["--runtime-managed-image"],
    "properties": {
      "golem.custom-runtime.enable" : false,
      "golem.custom-runtime.config" : {
        "value": 32
      }
    },
    "config": {
      "counters": {
        "golem.usage.network.in-mib": {
          "name": "in-network-traffic",
          "description": "Incoming network traffic usage in MiB",
          "price": true
        },
        "golem.usage.network.out-mib": {
          "name": "out-network-traffic",
          "description": "Outgoing network traffic usage in MiB",
          "price": true
        },
        "golem.usage.duration_sec": {
          "name": "duration",
          "description": "Activity duration in seconds",
          "price": true
        }
      }
    }
  }
]
```


| property        | optional | description                                                                                                                     |
|-----------------|----------|---------------------------------------------------------------------------------------------------------------------------------|
| name            | No       | Runtime name which will be placed in offer as `"golem.runtime.name"`                                                            |
| version         | No       | Runtime version following semantic versioning. Placed in Offer as `golem.runtime.version`                                       |
| supervisor-path | No       | Path to supervisor binary relative to this descriptor.                                                                          |
| runtime-path    | Yes      | Path to runtime binary relative to this descriptor.                                                                             |
| description     | Yes      | Human readable runtime description.                                                                                             |
| extra-args      | Yes      | Runtime specific arguments that will be appended to ExeUnit binary when starting.                                               |
| properties      | Yes      | Properties that will be attached to Offer. Dictionary with keys used as a path in Offer which value can be any legal json type. |
| config          | Yes      | Runtime configuration that can be used by Provider.                                                                             |
| config/counters | Yes      | Dictionary of supported usage counters.                                                                                         |
