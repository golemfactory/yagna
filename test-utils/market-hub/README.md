# Yagna Exchange Hub

For the Mk1 stage of project we will use centralised implementation of the
Yagna Market as specified [here](../../docs/market-api/market-api-mk0-central-exchange.md).

Below is the source repository, where you will find how to run it:

https://github.com/golemfactory/golem-client-mock/

It is written in C# on top of .NET Core, which beside Windows supports
also Ubuntu and macOS.

This implementation have no data persistence, which means all objects created
during the server process lifetime perishes after it is shut down.

It implements the [Market API](https://github.com/golemfactory/ya-client/blob/master/specs/market-api.yaml) and
conforms with Cabability Level 1 of the [Market API specification](
https://docs.google.com/document/d/1Zny_vfgWV-hcsKS7P-Kdr3Fb0dwfl-6T_cYKVQ9mkNg/edit#heading=h.8anq3nlk2en7
).

This mockup implements also the [Activity API](
https://github.com/golemfactory/ya-client/blob/master/specs/activity-api.yaml)
and conforms with Cabability Level 1 of the [Activity API specification](
https://docs.google.com/document/d/1BXaN32ediXdBHljEApmznSfbuudTU8TmvOmHKl0gmQM
).

## Offer-Demand matching

Offer-Demand matching is - in turn - implemented in Rust and used as interop
within aforementioned C# Market implementation.

Here are the sources for it
https://github.com/stranger80/golem-market-api

## Clients with basic Market

There are also sample Requestor and Provider mockups to interact with this Exchange Hub.

https://github.com/golemfactory/golem-architecture/tree/draft/projects/GolemSampleApp1

They have simple Market API and Activity API with dummy ExeUnit implementation.
Again, both are written in C# on top of the .NET Core.
