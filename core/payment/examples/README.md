## Payment API examples

### Startup

To start the API server (both provider & requestor) run the following commands:

```shell script
cd core/payment
cp ../../.env-template .env
(rm payment.db* || true) && cargo run --example payment_api
```

To use erc20 instead of Dummy driver
use `cargo run --example payment_api -- --driver=erc20 --platform=erc20-goerli-tglm` instead.

### Examples

To make sense of the included examples it is important to understand what parameters the example accepts and how they
can be used to change payment platform (driver, network, token). We list examples along with their parameters starting
from `payment_api` with is required to run any other example with payment platform that matches the `payment_api`'s
platform.

| Example             | Parameters                                               | Defaults                                                                                                          |
|---------------------|----------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------|
| payment_api         | driver, platform                                         | driver=`dummy`, platform=`dummy-glm`                                                                              |
| account_ballance    |                                                          | Same as `payment_api`                                                                                             |
| cancel_invoice      | driver, network                                          | driver=`dummy`, network=None                                                                                      |
| debit_note_flow     | platform                                                 | platform=`dummy-glm`                                                                                              |
| get_accounts        | <`provider_addr`><br/>  <`requestor_addr`><br/> platform | `provider_addr` and `requestor_addr` are required,  positional, `0x`-hex-encoded parameters. Platform=`dummy-glm` |
| invoice_flow        | platform                                                 | platform=`dummy-glm`                                                                                              |
| market_decoration   |                                                          | Same as `payment_api`                                                                                             |
| release_allocation  |                                                          | Same as `payment_api`                                                                                             |
| validate_allocation |                                                          | Same as `payment_api`                                                                                             |

<!-- Generated with https://www.tablesgenerator.com/markdown_tables -->
