### Preparing directories

Should randomize the port numbers

./prepare-local-setup.sh workdir/Provider1
./prepare-local-setup.sh workdir/Requestor1

#### Example directory structure

```
workdir/
├── Devnet1
│   └── yagna
├── Devnet2
│   └── yagna
├── exe-units
│   ├── bin
│   ├── lib
│   │   ├── plugins
│   │   │   └── ya-runtime-vm
│   │   │       └── runtime
│   │   └── plugins-dev
│   │       ├── extensions
│   │       └── ya-runtime-vm
│   │           └── runtime
│   └── ya-installer
│       ├── bundles
│       │   ├── golem-provider-linux-pre-rel-v0.5.0-495172ea
│       │   │   └── plugins
│       │   ├── ya-runtime-vm-linux-v0.2.0
│       │   │   └── ya-runtime-vm
│       │   │       └── runtime
│       │   ├── ya-runtime-vm-linux-v0.2.3
│       │   │   └── ya-runtime-vm
│       │   │       └── runtime
│       │   └── ya-runtime-wasi-linux-v0.2.1
│       └── terms
├── Provider1
│   ├── logs
│   ├── provider
│   │   ├── cert-dir
│   │   ├── exe-unit
│   │   │   ├── cache
│   │   │   └── work
│   │   │       └── logs
│   │   └── negotiations
│   └── yagna
├── Provider2
│   ├── provider
│   │   └── exe-unit
│   │       ├── cache
│   │       │   └── tmp
│   │       └── work
│   └── yagna
├── Provider3
│   ├── provider
│   │   ├── exe-unit
│   │   │   ├── cache
│   │   │   │   └── tmp
│   │   │   └── work
│   │   │       ├── 328eba5b3dae0e60d6eaf0447b761a2437f08c0a5e8b22fdc776caf044a64189
│   │   │       │   └── f4158a26e0eb4d82a482d4d7c742a5e2
│   │   │       │       ├── logs
│   │   │       │       └── vol-2370ab5e-ae9d-40a2-b683-d628e7dac32a
│   │   │       ├── 4b1321a8266efa42bda0fa5521ccf0a182de9cb15b810821f314e01903f3bffe
│   │   │       │   └── 3528ba1d83ab43e9baad013cbc9ab0e0
│   │   │       │       └── logs
│   │   │       └── logs
│   │   └── negotiations
│   └── yagna
├── Provider4
│   ├── logs
│   ├── provider
│   │   ├── cert-dir
│   │   └── exe-unit
│   │       ├── cache
│   │       └── work
│   │           └── logs
│   └── yagna
└── Requestor1
    ├── provider
    └── yagna
```
