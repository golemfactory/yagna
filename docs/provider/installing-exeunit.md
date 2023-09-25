# Installing ExeUnit

To install ExeUnit you need to do following steps:
1. Put ExeUnit binaries and descriptor in [directory](overview.md#provider-directories) used by Provider
2. Configure [preset](./../../agent/provider/readme.md#presets) that will be used to create Offer
3. Activate [preset](./../../agent/provider/readme.md#activating-and-deactivating-presets)

## Verifying if ExeUnit is visible for Agent 

You can list ExeUnits visible by Provider Agent using command
`ya-provider exe-unit list`

Here is example result for ExeUnits available in default installation:
```commandline
[2023-08-04T17:44:06.137+0200 INFO  ya_provider::execution::registry] Added [wasmtime] ExeUnit to registry. Supervisor path: [/home/nieznanysprawiciel/.local/lib/yagna/plugins/exe-unit], Runtime path: [Some("/home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-wasi")].
[2023-08-04T17:44:06.140+0200 INFO  ya_provider::execution::registry] Added [vm] ExeUnit to registry. Supervisor path: [/home/nieznanysprawiciel/.local/lib/yagna/plugins/exe-unit], Runtime path: [Some("/home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm")].
Available ExeUnits:

Name:          wasmtime
Version:       0.2.1
Supervisor:    /home/nieznanysprawiciel/.local/lib/yagna/plugins/exe-unit
Runtime:       /home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-wasi
Description:   wasmtime wasi runtime


Name:          vm
Version:       0.4.0
Supervisor:    /home/nieznanysprawiciel/.local/lib/yagna/plugins/exe-unit
Runtime:       /home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm
Description:   vm runtime
```

To check if presets are setup correctly reference [presets](./../../agent/provider/readme.md#presets) documentation.

### Running Agent to test ExeUnit

`golemsp run`

This command will start `yagna` daemon and `ya-provider` after REST endpoints will be ready.

There are 2 events in logs that you should look for:

#### Runtime self test execution

```commandline
[2023-08-04T18:09:07.282+0200 INFO  ya_provider::execution::registry] Testing runtime [vm]
[2023-08-04T18:09:07.282+0200 INFO  ya_provider::execution::exeunit_instance] Running ExeUnit: /home/nieznanysprawiciel/.local/lib/yagna/plugins/exe-unit
[2023-08-04 18:09:07.284069 +02:00] INFO [exe-unit/src/runtime/process.rs:105] Executing "/home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm" with ["test"] from path Ok("/home/nieznanysprawiciel/.local/share/ya-provider/exe-unit/work")
[2023-08-04 16:09:07.285482 +00:00] INFO [runtime/src/vmrt.rs:141] Executing command: Command { std: "/home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-vm/runtime/vmrt" "-m" "128M" "-nographic" "-vga" "none" "-kernel" "vmlinuz-virt" "-initrd" "initramfs.cpio.gz" "-enable-kvm" "-cpu" "host" "-smp" "1" "-append" "console=ttyS0 panic=1" "-device" "virtio-serial" "-device" "virtio-rng-pci" "-chardev" "socket,path=/tmp/d62a8ed8399249a3bc01e251ccfb7a98.sock,server=on,wait=off,id=manager_cdev" "-device" "virtserialport,chardev=manager_cdev,name=manager_port" "-drive" "file=/home/nieznanysprawiciel/.local/lib/yagna/plugins/ya-runtime-vm/runtime/self-test.gvmi,cache=unsafe,readonly=on,format=raw,if=virtio" "-no-reboot" "-net" "none" "-chardev" "socket,path=/tmp/d62a8ed8399249a3bc01e251ccfb7a98_vpn.sock,server,wait=off,id=vpn_cdev" "-device" "virtserialport,chardev=vpn_cdev,name=vpn_port" "-chardev" "socket,path=/tmp/d62a8ed8399249a3bc01e251ccfb7a98_inet.sock,server,wait=off,id=inet_cdev" "-device" "virtserialport,chardev=inet_cdev,name=inet_port", kill_on_drop: false }
[2023-08-04 16:09:07.285876 +00:00] INFO [runtime/src/guest_agent_comm.rs:459] Waiting for Guest Agent socket ...
```
If there is no error message then self-test execution passed.

#### Publishing Offer on market

```commandline
[2023-08-04T18:09:09.182+0200 INFO  ya_provider::market::provider_market] Creating offer for preset [vm] and ExeUnit [vm]. Usage coeffs: {"golem.usage.cpu_sec": 2.777777777777778e-5, "golem.usage.duration_sec": 5.555555555555556e-6}
[2023-08-04T18:09:09.183+0200 INFO  ya_provider::market::provider_market] Offer for preset: vm = {
  "properties": {
    "golem.activity.caps.transfer.protocol": [
      "gftp",
      "http",
      "https"
    ],
    "golem.com.payment.debit-notes.accept-timeout?": 240,
    "golem.com.payment.platform.erc20-mainnet-glm.address": "0xf98bb0842a7e744beedd291c98e7cd2c9b27f300",
    "golem.com.payment.platform.erc20-polygon-glm.address": "0xf98bb0842a7e744beedd291c98e7cd2c9b27f300",
    "golem.com.payment.platform.zksync-mainnet-glm.address": "0xf98bb0842a7e744beedd291c98e7cd2c9b27f300",
    "golem.com.pricing.model": "linear",
    "golem.com.pricing.model.linear.coeffs": [
      0.00002777777777777778,
      5.555555555555556e-6,
      0.0
    ],
    "golem.com.scheme": "payu",
    "golem.com.scheme.payu.debit-note.interval-sec?": 120,
    "golem.com.scheme.payu.payment-timeout-sec?": 120,
    "golem.com.usage.vector": [
      "golem.usage.cpu_sec",
      "golem.usage.duration_sec"
    ],
    "golem.inf.cpu.architecture": "x86_64",
    "golem.inf.cpu.brand": "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz",
    "golem.inf.cpu.capabilities": [
        ...
    ],
    "golem.inf.cpu.cores": 6,
    "golem.inf.cpu.model": "Stepping 10 Family 6 Model 302",
    "golem.inf.cpu.threads": 11,
    "golem.inf.cpu.vendor": "GenuineIntel",
    "golem.inf.mem.gib": 21.33261077105999,
    "golem.inf.storage.gib": 28.130107879638672,
    "golem.node.debug.subnet": "public",
    "golem.node.id.name": "leopard",
    "golem.node.net.is-public": true,
    "golem.runtime.capabilities": [
      "inet",
      "vpn",
      "manifest-support",
      "start-entrypoint"
    ],
    "golem.runtime.name": "vm",
    "golem.runtime.version": "0.3.0",
    "golem.srv.caps.multi-activity": true,
    "golem.srv.caps.payload-manifest": true
  },
  "constraints": "(&\n  (golem.srv.comp.expiration>1691165349182)\n  (golem.node.debug.subnet=public)\n)"
}
[2023-08-04T18:09:09.183+0200 INFO  ya_provider::market::provider_market] Subscribing to events... [vm]
[2023-08-04T18:09:09.183+0200 INFO  ya_market::matcher] Subscribed new Offer: [bd4719b0944e4e718eb3dac3745102bf-18c9ab5179dfa0a66b3d1d0aaa25f841569cc76b977ecf618f308d51a3b92f52] using identity: golem-cli [0x4f597d426bc06ed463cd2639cd5451667f9c3e3d]
[2023-08-04T18:09:09.183+0200 INFO  ya_provider::market::provider_market] Subscribed offer. Subscription id [bd4719b0944e4e718eb3dac3745102bf-18c9ab5179dfa0a66b3d1d0aaa25f841569cc76b977ecf618f308d51a3b92f52], preset [vm].
```
