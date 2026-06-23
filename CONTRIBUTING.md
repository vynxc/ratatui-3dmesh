# Contributing

Thanks for helping improve `ratatui-3dmesh`.

## Development

```bash
cargo fmt --all
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --all-features --no-deps
```

## Model assets

Only add sample assets that are public domain, MIT/Apache compatible, or explicitly redistributable. Include attribution and license notes in the pull request.

## Scope

The crate focuses on embeddable Ratatui rendering. Standalone terminal lifecycle code should stay in examples or downstream apps.
