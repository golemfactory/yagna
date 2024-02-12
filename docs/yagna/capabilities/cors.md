# Cors headers

Why is it needed:

- local tests by developers.
- convenient connection of applications in public arenas with the local golem node.

### Examples

#### Erigon

**--http.corsdomain value**
Comma separated list of domains from which to accept cross origin requests (browser enforced)

**--http.vhosts value**
Comma separated list of virtual hostnames from which to accept requests (server enforced). Accepts '*' wildcard. (default: "localhost")

#### Lightouse

***--http-allow-origin**
`<ORIGIN>`  this server (e.g., http://localhost:5052).

## Yagna Requirements

### Ability to define default cors rules

Add new commandline argument api-allow-origin (defined by env YAGNA_API_ALLOW_ORIGIN).


```
$ yagna service run --api-allow-origin='*'
```


### Rules per appkey

Adding new appkey

```
$ yagna app-key create --id 0x578349a0d1dd825162fe8579a51efa220a9f4b17 --allow-origin https://dapps.golem.network dapp-portal
4f1cf7c363e9403b9ce15823cec182ff
$ yagna app-key list
┌───────────────┬──────────────────────────────────────────────┬───────────┬─────────────────────────────────┐
│  name         │  id                                          │  role     │  created                        │
├───────────────┼──────────────────────────────────────────────┼───────────┼─────────────────────────────────┤
│  dapp-portal  │  0x578349a0d1dd825162fe8579a51efa220a9f4b17  │  manager  │  2022-12-01T14:11:20.113596023  │
└───────────────┴──────────────────────────────────────────────┴───────────┴─────────────────────────────────┘
$ yagna app-key show dapp-portal
---
name: dapp-portal
key: 4f1cf7c363e9403b9ce15823cec182ff
id: "0x578349a0d1dd825162fe8579a51efa220a9f4b17"
role: manager
created: 2022-12-01T14:11:20.113596023
allowOrigin:
   - https://dapps.golem.network
---



