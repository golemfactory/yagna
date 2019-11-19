# Project YAGNA

## Purpose of new solution

* Creation of production-ready, easy to maintain architecture and code base.
* Improved onboarding and UX so we won’t lose significant number of users during installation phase.
* Keep solution tokenocentric.

## Requirements

### Functional 
1. Implementing “Golem” functionalities, ie. distributed computations
    * [ ] __Batching__
    * [ ] Services _(optional)_
2. Computational environment
   * [ ] __Wasm computation__
   * [ ] Light vm-s _(optional)_
   * [ ] Docker on Linux _(optional)_
3. Payment platform
    * [ ] Payments with GNT
    * [ ] Gasless transactions
    * [ ] ERC20 token
    * [ ] payment matching
4. Modular transaction system
    * [ ] Possibility to easily change transaction system for app
    * [ ] Usage market
    * [ ] Pay per task
    * [ ] Pay for dev _(optional)_
5. Network
    * [ ] P2P (Hybrid P2P) 
    * [ ] Ability to work behind NAT (Relays)
6. Modular verification
    * [ ] Possibility to easily change verification method for app
    * [ ] Verification by redundancy
    * [ ] No verification
    * [ ] Verification by human _(optional)_
7. Back compatibility
    * [ ] Gateway between Golem Brass

## Project layout

* docs - implementation specs & other documents.
* 