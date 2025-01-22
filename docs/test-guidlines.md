# Testing Guidelines

## Test suites

Current implementation has following test suites:

- [Goth](../goth_tests/README.md) - Integration tests framework allowing to create test network and scenarios simulating
  Requestor behavior
- [Market Test Suite](../core/market/readme.md) - Test suite for testing Market module in separation
- System Tests - Testing yagna modules as semi-integration tests with mocking some parts of yagna daemon
    - [Payments Tests](../core/payment/tests) - Semi-integration tests for payment module
    - [Identity Tests](../core/identity/tests) - Semi-integration tests for identity module
    - [ExeUnit Tests](../exe-unit/tests) - Tests for ExeUnit module in separation from yagna
    - [ExeUnit components tests](../exe-unit/components/transfer/tests) - Tests for ExeUnit components (here mainly for
      transfer)
- [Yagna integration framework](../tests/readme.md) - Testing framework that was an attempt to provide alternative way
  to create integration tests closer to yagna. It is not mature yet.
- [Provider Agent Tests](../agent/provider/tests) - Tests for Provider Agent functionalities (Excluding test cases where
  yagna is required)
- [Additional payments tests](../extra/payments/multi_test)
- Network Tests - ya-relay-client tests
    - [Functional testing environment](https://github.com/golemfactory/ya-relay/tree/main/tests_integration)
    - [Library integration tests](https://github.com/golemfactory/ya-relay/tree/main/tests)

## Testing tools

- [Goth](https://github.com/golemfactory/goth) - Integration testing framework for Golem
- [Semi-integration tests framework and utils](https://github.com/golemfactory/yagna/tree/master/test-utils/test-framework) -
  Common tools and mocks used by System Tests
- [Provider test utils](../utils/manifest-utils/test-utils) - Utils for testing Provider Agent functionalities and
  creating certificates manifests etc.
- [ya-perf](https://github.com/golemfactory/ya-perf) - Network performance measuring scripts

## Other tools

- [Deploying test networks](https://github.com/golemfactory/yagna-testnet-scripts/blob/master/ansible/README.md) -
  ansible scripts for deploying test networks