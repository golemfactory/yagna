# Project YAGNA

![CI](https://github.com/golemfactory/yagna/workflows/CI/badge.svg)

An open platform and marketplace for distributed computations.

## Project Layout

* [core](core) - core services for open computation market.
* [agent](agent) - applications based on core services. In MVP there are
basic wasm provider and simple wasm requestor.
* [exe-unit](exe-unit) -  execution environments implementation. For MVP there are:
    * [dummy](exe-unit/dummy) - mock exe unit for tests, and one of below:
    * [wasm-mozjs](exe-unit/wasm-mozjs) - [SpiderMonkey](https://github.com/servo/rust-mozjs) based provider.
    * [wasmtime](exe-unit/wasmtime) - [Wasmtime](https://github.com/bytecodealliance/wasmtime) based provider.
* [interfaces](interfaces) - public API for core services and data model.
* [service-bus](service-bus) - portable, rust-oriented service bus for IPC.
* [docs](docs) - project documentation including analysis and specifications.

## Requirements

* Clean and easy UX, especially during onboarding.
* Tokenocentric (GNT).
* Production-ready, modular and easy to maintain architecture and code base.  
_Modular_ means that all building blocks are to be easily replaceable.
* Documentation and SDK for developers.
* Binaries with small footprint.

### Functional 

1. Distributed computations
    * [ ] **Batching**
    * [ ] Services _(optional)_
1. Computational environment
   * [ ] **Wasm computation**
   * [ ] Light vm-s _(optional)_
   * [ ] Docker on Linux _(optional)_
1. Payment platform
    * [ ] **Payments with GNT**
    * [ ] **Gasless transactions**
    * [ ] **ERC20 token**
    * [ ] payment matching _(optional)_
1. Transaction system
    * [ ] **Usage market**
    * [ ] **Pay per task**
    * [ ] Pay for dev _(optional)_
1. Network
    * [ ] **P2P** (Hybrid P2P) 
    * [ ] **Ability to work behind NAT** (Relays)
1. Verification
    * [ ] **Verification by redundancy**
    * [ ] **No verification**
    * [ ] Verification by human _(optional)_
1. Back compatibility
    * [ ] Golem Brass interoperability _(optional)_

## References

- [MVP Requirements](https://docs.google.com/document/d/1GZnZ725E_OIRkXzYJNlmafNGDDvR88LFaDpzAmio_nQ)
- [Technical Concept](https://docs.google.com/document/d/1Sdk-N_CmsXcxpXi1dQVSmbiQwxMF3w1nF82Xv0Vjw08)
