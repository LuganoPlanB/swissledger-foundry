# Foundry & Cast — Compatibility Notes for ledger.swiss

This document catalogues every limitation encountered when using the Foundry
toolchain (`forge`, `cast`) against the SwissLedger chain (id 110). It is
written as a specification for a downstream fork that avoids the need for an
intermediate RPC proxy.

The reference version is **Foundry 1.7.1** (commit `4072e48705`).

---

## 1. Missing `params` field in JSON-RPC requests

### Behaviour

`cast` and `forge` omit the `"params"` key for methods that take no parameters.
Per JSON-RPC 2.0 this is valid, but the ledger.swiss Blockscout RPC endpoint
(`/api/eth-rpc`) requires it.

### Affected methods

All zero-parameter RPC methods:

- `eth_blockNumber`
- `eth_chainId`
- `eth_gasPrice`
- `eth_maxPriorityFeePerGas`

Methods that technically have parameters but also fail when cast sends them
without `params`:

- `eth_estimateGas` — cast sends a transaction object; server rejects when
  `params` is missing.
- `eth_call` — cast sends a transaction object and block tag; same issue.

### Error message

```
deserialization error: invalid type: string "Method, params, and jsonrpc,
are all required parameters.", expected a JSON-RPC 2.0 error object
```

### Reproduction

```bash
cast block-number --rpc-url https://explorer.ledger.swiss/api/eth-rpc
cast chain-id    --rpc-url https://explorer.ledger.swiss/api/eth-rpc
```

### What cast sends (broken)

```json
{"method":"eth_blockNumber","id":0,"jsonrpc":"2.0"}
```

### What the server expects (working)

```json
{"method":"eth_blockNumber","params":[],"id":0,"jsonrpc":"2.0"}
```

### Fork TODO

Always emit `"params"` for every JSON-RPC request. For methods with no
arguments, emit `"params":[]`. For convenience, a configuration option
(`--require-params` or env variable `ETH_RPC_REQUIRE_PARAMS=true`) could
allow users to opt in at runtime.

The relevant code is likely in `crates/cast/bin/cmd/` (CLI commands that
build JSON-RPC bodies) and in the shared RPC transport layer.

---

## 2. Non-standard error response format

### Behaviour

When the server returns an error, it uses a plain string instead of the
JSON-RPC 2.0 error object:

```json
{"jsonrpc":"2.0","error": "Method, params, and jsonrpc, are all required parameters.","id": 0}
```

Standard JSON-RPC 2.0 errors must be:

```json
{"jsonrpc":"2.0","error": {"code": -32600, "message": "Invalid Request"}, "id": 0}
```

### Error message

```
deserialization error: invalid type: string "…", expected a JSON-RPC 2.0
error object at line 1 column 85
```

This causes a hard crash in cast/forge because the Rust deserialiser expects
`error` to be an object with `code` and `message` fields.

### Affected operations

Any RPC call that the server rejects with a non-standard error. Specific
examples:

- `eth_gasPrice` → `"Internal server error"` (string)
- Any zero-param method without `params` → `"Method, params, and jsonrpc, are all required parameters."` (string)

### Fork TODO

Make the `error` field deserialisation lenient. Accept both:
- Standard: `{"code": int, "message": string, "data": ...}`
- Non-standard plain string

When a plain-string error is received, wrap it as:
```json
{"code": -32000, "message": "<string content>"}
```

Alternatively, accept a raw `serde_json::Value` for the error field and
normalise after parsing.

The relevant code is likely in the `alloy-json-rpc` or `alloy-transport`
crate, in the `RpcError` or `ErrorPayload` types.

---

## 3. CLI `--evm-version` flag does not override `foundry.toml`

### Behaviour

`forge build --evm-version london` ignores the CLI flag when `foundry.toml`
sets `evm_version = "cancun"` in `[profile.default]`. The build uses the
config file's value regardless.

### Reproduction

```bash
# foundry.toml has evm_version = "cancun"
forge build --evm-version london
forge inspect MerkleRootRegistry bytecode  # still has PUSH0 opcodes
```

### What works

Changing `[profile.default].evm_version` directly works. But creating a
separate `[profile.london]` with `evm_version = "london"` and setting
`FOUNDRY_PROFILE=london` also works — as long as the profile has all
required fields (`solc_version`, `optimizer`, `via_ir`, etc.), because
profiles do not inherit from `[profile.default]`.

### Fork TODO

Ensure `--evm-version <VERSION>` on the `forge build` command line takes
precedence over whatever the config file says. This is the standard CLI
override pattern and should not be silently ignored.

Also consider making profiles inherit from `[profile.default]` for any
keys they do not explicitly set, so that a profile only needs to override
the fields it changes.

---

## 4. `cast call` / `cast calldata` cannot parse non-empty `bytes32[]` arguments

### Behaviour

When passing a dynamic `bytes32[]` argument to `cast call` or `cast calldata`,
the argument parser fails for any non-empty array. An empty array `[]` works.

### Error message (bracket syntax, e.g. `[0xabcd...]`)

```
Error: parser error:
[0xdf02a603c991a0617e6daf13d208ef96890f7d59d3d8a3f73ae24234be6737]
                                                                 ^
invalid string length
```

### Error message (space-separated individual arguments)

```
Error: parser error:
0xdf02a603c991a0617e6daf13d208ef96890f7d59d3d8a3f73ae24234be6737
^
expected `[`
```

### Error message (JSON-style quoted elements)

```
Error: parser error:
["0xdf02a603c991a0617e6daf13d208ef96890f7d59d3d8a3f73ae24234be6737"]
 ^
expected hex digits or the `0x` prefix for an empty hex string
```

### Reproduction

```bash
cast call 0x... "containsLeafHash(bytes32,bytes32[])(bool)" \
  0xleafhash...
  "[0xdf02a603c991a0617e6daf13d208ef96890f7d59d3d8a3f73ae24234be6737]"
```

The same failure occurs with `cast calldata`, `cast abi-encode`, and any
other command that parses dynamic `bytes32[]` arguments.

### Workaround

Pre-encode the calldata externally (e.g. with `@ethersproject/abi`) and
pass the raw hex to `cast call`:

```bash
cast call 0x... 0x000aa4d2...
```

Note that `cast call` with raw calldata returns the raw ABI-encoded return
value (e.g. `0x00...01` for `true`), not the decoded human-readable form.

### Fork TODO

Fix the argument parser for dynamic arrays. The parser appears to be in the
CLI argument handling layer (`cast` argument parsing → type casting → ABI
encoding). The issue is specific to `bytes32[]` — the parser likely
mishandles the hex string length validation for elements inside a dynamic
array. Check the type-coercion path for array arguments in the cast CLI.

---

## 5. `eth_gasPrice` returns "Internal server error"

### Behaviour

The server's `eth_gasPrice` implementation is incomplete. It returns an HTTP
500 with the body:

```
"Internal server error"
```

Combined with issue #2 (non-standard error format), this causes a
deserialisation crash.

### Fork TODO

Two-pronged:
1. Accept non-standard error strings (covered by #2).
2. Treat a failed `eth_gasPrice` with a 500 or error response as
   equivalent to `0x0` when the caller has explicitly passed
   `--legacy --gas-price 0`.  A chain that errors on `eth_gasPrice` is
   signalling that gas price is irrelevant (either free or handled
   server-side).  The tool should fall back to 0 and proceed.

---

## 6. `eth_estimateGas` returns "Incorrect number of params"

### Behaviour

Even with a well-formed request including `"params"`, the RPC endpoint
rejects `eth_estimateGas` with:

```json
{"jsonrpc":"2.0","error": "Incorrect number of params.","id": 1}
```

The exact cause is unclear — it may be that the server's `eth_estimateGas`
implementation expects a different transaction object shape (e.g. requiring
or forbidding certain fields, or not supporting the second optional `block`
parameter).

### Fork TODO

If `eth_estimateGas` fails with a non-retryable error, the tool should fall
back to the user-provided `--gas-limit` (or a sensible default).  Do not
treat a missing estimate as a fatal error when the user explicitly set a
gas limit.

---

## 7. Chain only supports legacy (type 0) transactions

### Behaviour

Sending an EIP-1559 (type 2) or EIP-2930 (type 1) transaction is rejected:

```json
{"jsonrpc":"2.0","error":{"code":-32000,"message":"transaction type not supported"},"id":1}
```

### Fork TODO

Already handled by `--legacy`. No fork changes needed.

---

## 8. Chain rejects non-zero gas price

### Behaviour

Any transaction with `gasPrice > 0` is rejected:

```json
{"jsonrpc":"2.0","error":{"code":-32000,"message":"Gas price not 0"},"id":1}
```

### Fork TODO

When `eth_gasPrice` returns an error (see #5) and the user hasn't set an
explicit gas price, default to `0` instead of erroring out. This avoids
the need for users to pass `--gas-price 0` manually on gas-free chains.

---

## 9. Block gas limit

| Property | Value |
|---|---|
| Block gas limit | 20,000,000 |
| `MerkleRootRegistry` deployment | ~2,000,000 |

Not a Foundry issue, but relevant for context.

---

## 10. Proxy BrokenPipe errors (indirect)

### Behaviour

The Python-based RPC proxy (`scripts/rpc-proxy.py`) gets `BrokenPipeError`
when `cast call` closes the HTTP connection before the proxy finishes writing
the response body. This is due to `cast` not reading the full response in
some code paths (possibly an optimisation where the body is discarded after
the status line or headers).

### Fork TODO

Ensure the HTTP transport always reads the full response body before closing
the connection, or at minimum issues a `Connection: close` and clean
shutdown. This is a minor issue but contributes to proxy fragility.

---

## Summary of fork changes

| # | Priority | Area | Change |
|---|---|---|---|
| 1 | **P0** | JSON-RPC transport | Always emit `"params":[]` for zero-param methods |
| 2 | **P0** | JSON-RPC deserialisation | Accept plain-string error responses |
| 3 | P1 | `forge build` CLI | Honour `--evm-version` over config file |
| 4 | P1 | `cast` argument parser | Fix `bytes32[]` array parsing |
| 5 | P2 | `cast` gas logic | Fall back to 0 gas price on `eth_gasPrice` error |
| 6 | P2 | `cast` simulation | Fall back to user gas limit on `eth_estimateGas` error |
| 7 | P0 | (already works) | Legacy transaction type |
| 8 | P2 | `cast` gas logic | Default to 0 gas price when chain errors |
| 10 | P3 | HTTP transport | Always read full response before closing |

Fixing P0 items (#1 and #2) would eliminate the need for the RPC proxy
entirely. P1 items are quality-of-life improvements. P2 items would make
`--gas-price 0 --legacy --gas-limit N` unnecessary on gas-free chains.
