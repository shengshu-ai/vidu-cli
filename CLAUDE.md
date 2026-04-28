# vidu-cli

## Build

```bash
cargo build            # debug
cargo build --release  # release
```

## Test

```bash
# Unit + integration tests (no token needed)
cargo test

# E2E tests (requires VIDU_TOKEN)
source test/env.sh && cargo test -- --include-ignored

# Run only E2E tests
source test/env.sh && cargo test --test e2e -- --ignored
```

## Coverage

```bash
# Unit + integration only
cargo llvm-cov --summary-only -- --include-ignored

# With E2E (requires VIDU_TOKEN)
source test/env.sh && cargo llvm-cov --summary-only -- --include-ignored
```

Requires `cargo-llvm-cov` (`cargo install cargo-llvm-cov`) and `rustup component add llvm-tools-preview`.
