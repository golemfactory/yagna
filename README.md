# Project YAGNA

An open platform and marketplace for distributed computations.

## Project Layout

* core - core services for open computation market.
* agent - applications based on core services. In MVP there is
basic wasm provider and simple wasm requestor.
* exe-unit -  execution enviromens implementation. for MVP there is:
    * dummy - mock exe unit for tests.
    * wasmtime - wasmtime based provicder.
* interfaces - public API for core services.
* service-bus - portable, rust-oriented service bus for IPC.

## Requirements

* Clean and easy UX, especially during onboarding.
* Tokenocentric (GNT).
* Production-ready, modular and easy to maintain architecture and code base.  
_Modular_ means that all building blocks are to be easily replaceable.
* Documentation and SDK for developers.
* Binaries with small footprint.

### Functional 

1. Distributed computations
    * [ ] __Batching__
    * [ ] Services _(optional)_
1. Computational environment
   * [ ] __Wasm computation__
   * [ ] Light vm-s _(optional)_
   * [ ] Docker on Linux _(optional)_
1. Payment platform
    * [ ] __Payments with GNT__
    * [ ] __Gasless transactions__
    * [ ] __ERC20 token__
    * [ ] payment matching _(optional)_
1. Transaction system
    * [ ] __Usage market__
    * [ ] __Pay per task__
    * [ ] Pay for dev _(optional)_
1. Network
    * [ ] __P2P__ (Hybrid P2P) 
    * [ ] __Ability to work behind NAT__ (Relays)
1. Verification
    * [ ] __Verification by redundancy__
    * [ ] __No verification__
    * [ ] Verification by human _(optional)_
1. Back compatibility
    * [ ] Golem Brass interoperability _(optional)_

## References

- [MVP Requirements](https://docs.google.com/document/d/1GZnZ725E_OIRkXzYJNlmafNGDDvR88LFaDpzAmio_nQ)
- [Technical Concept](https://docs.google.com/document/d/1Sdk-N_CmsXcxpXi1dQVSmbiQwxMF3w1nF82Xv0Vjw08)
- [Technical Analysis docs](https://github.com/golemfactory/golem-architecture/tree/draft/docs) 
