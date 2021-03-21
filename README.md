## Golem

Official Rust implementation of Golem. Golem is a network of nodes that implement the Golem Network protocol. We provide the default implementation of such a node in the form of the Golem daemon, Yagna.

<h5 align="center">
  <a href='https://golem.network/'><img
      width='500px'
      alt=''
      src="https://user-images.githubusercontent.com/35585644/111472751-939f5100-872a-11eb-8c26-926117080e35.png" /></a>
  <br/>A flexible, open-source platform for democratised access to digital resources.
</a>
</h5>

</p>
<p align="center">
    <a href="https://github.com/golemfactory/yagna/workflows/CI/badge.svg" alt="CI">
        <img src="https://github.com/golemfactory/yagna/workflows/CI/badge.svg" /></a>
    <a href="https://github.com/golemfactory/yagna/watchers" alt="Watch on GitHub">
        <img src="https://img.shields.io/github/watchers/golemfactory/yagna.svg?style=social" /></a>
    <a href="https://github.com/golemfactory/yagna/stargazers" alt="Star on GitHub">
        <img src="https://img.shields.io/github/stars/golemfactory/yagna.svg?style=social" /></a>
    <a href="https://discord.gg/y29dtcM" alt="Discord">
        <img src="https://img.shields.io/discord/684703559954333727?logo=discord" /></a>
    <a href="https://twitter.com/golemproject" alt="Twitter">
        <img src="https://img.shields.io/twitter/follow/golemproject?style=social" /></a>
    <a href="https://reddit.com/GolemProject" alt="Reddit">
        <img src="https://img.shields.io/reddit/subreddit-subscribers/GolemProject?style=social" /></a>
</p>

Golem Network has officially gone on mainnet with the [Beta I release](https://blog.golemproject.net/mainnet-release-beta-i/).

Golem democratizes society’s access to computing power by creating a decentralized platform where anyone can build a variety of applications, request computational resources and/or offer their idle systems in exchange for cryptocurrency tokens (GLM). The actors in this decentralized network can assume one of the three non-exclusive roles:

* **Requestor**
Has a need to use IT resources such as computation hardware. Those resources are purchased in the decentralized market. The actual usage of the resources is backed by Golem's decentralized infrastructure.

* **Provider**
Has IT resources available that can be shared with other actors in the network. Those resources are sold in the decentralized market.

* **Developer**
Builds applications to run for requestors on the network. Golem's potential goes much beyond a singular application. See [Awesome Golem](https://github.com/golemfactory/awesome-golem/blob/main/README.md#%EF%B8%8F-apps) for just a taste of the various types of applications that can be built and run on Golem!

## Documentation
For a more in-depth look at how Golem works, head over to our [documentation.](https://handbook.golem.network/)

## Project Layout

* [agent](agent) - basic agent applications based on core services. 
* [core](core) - core services for the open computation marketplace.
* [exe-unit](exe-unit) -  ExeUnit Supervisor.
* [service-bus](service-bus) - portable, rust-oriented service bus for IPC.
* [test-utils](test-utils) - some helpers for testing purposes
* [utils](utils) - trash bin for all other stuff ;)
* [docs](docs) - project documentation including analysis and specifications.

## Public API
The public API rust binding with data model is in the 
[ya-client](https://github.com/golemfactory/ya-client) repo.

## High Level APIs
The public high-level API for Python is in 
[yapapi](https://github.com/golemfactory/yapapi) repo and the JS/TS port is contained in the [yaJSapi](https://github.com/golemfactory/yajsapi) repo.

## Runtimes
We call our runtime **ExeUnit**. As for now we support
 * [Light VM](https://github.com/golemfactory/ya-runtime-vm) - [QEMU](https://www.qemu.org/)\-based ExeUnit.
 * and WASM in two flavours:
   * [wasmtime](https://github.com/golemfactory/ya-runtime-wasi) - [Wasmtime](https://github.com/bytecodealliance/wasmtime)\-based ExeUnit.
   * [emscripten](https://github.com/golemfactory/ya-runtime-emscripten) - [SpiderMonkey](https://github.com/servo/rust-mozjs)\-based ExeUnit.

Other ExeUnit types are to come (see below).

## MVP

With the MVP out, in the form of the [Beta I, Grace Hopper](https://github.com/golemfactory/yagna/releases/tag/v0.6.1) release. The release includes:
* Clean and easy UX, most specifically during onboarding.
* GLM-centric.
* Production-ready, modular and easy to maintain architecture and code base.  
_Modular_ means that all the building blocks can be easily replaceable.
* Documentation and SDK for developers.
* Small footprint binaries.

### Functional 

1. Distributed computations
    * [x] **Batching**
    * [ ] Services _(optional)_
1. Computational environment (aka ExeUnit)
   * [x] **Wasm computation**
   * [x] Light vm-s _(optional)_
   * [ ] Docker on Linux _(optional)_
   * [ ] SGX on Graphene _(optional)_
1. Payment platform
    * [x] **Payments with GLM**
    * [x] **Gasless transactions**
    * [x] **ERC20 token**
    * [ ] payment matching _(optional)_
1. Transaction system
    * [x] **Usage market**
    * [x] **Pay per task**
    * [ ] Pay for dev _(optional)_
1. Network
    * [ ] **P2P** (Hybrid P2P) 
    * [ ] **Ability to work behind NAT** (Relays)
1. Verification
    * [ ] **Verification by redundancy**
    * [x] **No verification**
    * [ ] Verification by humans _(optional)_
1. Back compatibility
    * [ ] Golem Brass/Clay interoperability _(optional)_
