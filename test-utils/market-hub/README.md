# Centralized Market Exchange

For the first stage of integration testing we will use
GolemClientMockAPI written in C# on top of .NET Core,
which beside Windows supports also Ubuntu and macOS.

https://github.com/stranger80/golem-client-mock/

It is a centralised implementation of the Market exchange
fully supporting
[Market API](../../interfaces/specs/market-api.yaml),
using non persistent (in-memory) repositories.

## Offer-Demand matching

Offer-Demand matching is implemented in Rust and used as intorop within C#

https://github.com/stranger80/golem-market-api
