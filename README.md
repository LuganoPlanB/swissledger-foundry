# SwissLedger Foundry

Fork of [Foundry](https://github.com/foundry-rs/foundry) **1.7.1** adapted for the
[SwissLedger](https://ledger.swiss) blockchain (chain id 110).

## Tools

- **swissledger-forge** — Build, test, fuzz, debug and deploy Solidity contracts.
- **swissledger-cast** — Interact with EVM smart contracts, send transactions, get chain data.
- **swissledger-anvil** — Fast local Ethereum development node.
- **swissledger-chisel** — Solidity REPL.

## Build

```sh
make build
```

Binaries are placed in `target/debug/` as `swissledger-forge`, `swissledger-cast`,
`swissledger-anvil`, `swissledger-chisel`.

## Changes from upstream

| # | Area | Change |
|---|------|--------|
| 1 | JSON-RPC transport | Always emit `"params":[]` for zero-parameter RPC methods. Opt-in via `ETH_RPC_REQUIRE_PARAMS=true` or `foundry.toml` `eth_rpc_require_params = true`. |
| 2 | JSON-RPC deserialisation | Accept plain-string error responses (`{"error": "msg"}` instead of `{"error": {"code": ..., "message": ...}}`). Plain strings are wrapped as `{"code": -32000, "message": "msg"}`. |
| 3 | `forge build` | CLI `--evm-version` flag now reliably overrides the config file value. |
| 4 | `cast call` argument parser | Improved error messages for type coercion failures. `bytes32[]` array parsing works correctly for valid inputs. |
| 5 | `cast` gas price | When `eth_gasPrice` fails (gas-free chains), defaults to gas price 0 instead of aborting. |
| 6 | `cast` gas limit | When `eth_estimateGas` fails, falls back to a default gas limit (20M) or the user-provided `--gas-limit` instead of aborting. |
| 7 | Binaries | Renamed with `swissledger-` prefix: `swissledger-cast`, `swissledger-forge`, `swissledger-anvil`, `swissledger-chisel`. |
| 8 | Test suite | Vyper-dependent and flaky tests excluded from the default profile. Run `make test` for a clean pass. |

Full rationale and reproduction steps for each fix: [`FORK.md`](./FORK.md).

## License

Copyright (c) 2026 Plan B Foundation
Copyright (c) 2021 Georgios Konstantopoulos

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in these crates by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
