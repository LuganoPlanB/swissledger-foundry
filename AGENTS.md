# AGENTS.md — Foundry

Rust workspace (edition 2024, MSRV 1.89) for the Ethereum development toolkit.
Four CLIs: `forge`, `cast`, `anvil`, `chisel`.

## Build & test commands

```sh
make build              # cargo build --locked --features "jemalloc aws-kms ..."
make test               # unit (nextest) + doc tests
make test-unit          # cargo nextest run --workspace --locked -E 'kind(test) & !test(/\b(issue|ext_integration|flaky_)/)'
make test-doc           # cargo test --doc --workspace --locked
make lint               # nightly fmt + clippy + typos
make pr                 # full CI gate: deny → lint → test → doc
make check              # cargo hack check --feature-powerset --depth 1
make doc                # nightly rustdoc with --document-private-items
```

Use `make build` / `make pr` — these encapsulate all feature flags and toolchains correctly.

## Test runner: `cargo nextest`

Tests use `cargo nextest`, not `cargo test`. Config at `.config/nextest.toml`:

- **Flaky tests** named `flaky_*` are excluded from the default profile.
  Run them with: `cargo nextest run --profile flaky`
- **Ext integration tests** named `ext_integration_*` get extended slow-timeout (5m).
- **Cheatcodes spec** package (`foundry-cheatcodes-spec`) has retries=0.

## Formatting: nightly rustfmt required

Run `cargo +nightly fmt`. The `rustfmt.toml` uses nightly-only options
(`max_width`, `comment_width`, `imports_granularity = "Crate"`, etc.).
Stable rustfmt will produce different output.

Non-Rust files (JSON, TOML, YAML, Markdown, Dockerfile, TypeScript) use `dprint`.
Run `make dprint-fmt` or `dprint fmt`. CI checks both rustfmt and dprint.

## Print macros are disallowed

`std::print`, `std::println`, `std::eprint`, `std::eprintln` are banned via clippy.
Use `sh_print!`, `sh_println!`, `sh_eprint!`, `sh_eprintln!` from `foundry_common::shell` instead.

## Clippy

```sh
cargo +nightly clippy --workspace --all-targets --all-features --locked -- -D warnings
```

## Key crates

| Crate | What |
|---|---|
| `crates/forge/` | Build, test, fuzz, debug, deploy Solidity contracts |
| `crates/cast/` | EVM RPC interaction, transactions, chain data |
| `crates/anvil/` | Local Ethereum dev node |
| `crates/chisel/` | Solidity REPL |
| `crates/cli/` | Shared CLI code for forge and cast |
| `crates/config/` | All Foundry settings / configuration |
| `crates/evm/evm/` | EVM execution engine (wraps revm) |
| `crates/evm/fuzz/` | Fuzzing engine |
| `crates/cheatcodes/` | Solidity cheatcodes for testing |
| `crates/test-utils/` | Internal test harness: `TestProject`, `TestCommand`, `ScriptTester` |
| `crates/cheatcodes/spec/` | Cheatcode specifications (auto-generated code) |

## Tests

- Tests that use forking must contain `fork` in their name.
- Integration tests use snapbox for snapshot testing; `crates/test-utils/` provides
  `TestProject`, `TestCommand`, `ScriptTester`, `ExtTester` as harness helpers.
- `testdata/` contains Solidity fixtures used by tests; not a valid Foundry project.
- Solidity test files in `testdata/` must be formatted with `forge fmt`.
  Run `make fmt` to format both Rust and Solidity.

## Debugging

The dev profile strips debug info for faster builds (`debug = "line-tables-only"`).
To use a debugger with full debug info, uncomment the dev profile section in `Cargo.toml`
lines ~134-137 or override locally with `CARGO_PROFILE_DEV_DEBUG=2`.

## Environment

- `mise.toml` sets `rust = "latest"` (use `mise install`).
- `flake.nix` provides a Nix dev shell with all tool dependencies.
- CI requires `solc`, `vyper`, `dprint`, `nodejs` for full test coverage.
