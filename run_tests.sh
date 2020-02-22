# Display verbose backtrace for debugging
export RUST_BACKTRACE=full

# Format check
cargo fmt --all -- --check

# Lints check
cargo clippy -- -D warnings

# Regular Tests
cargo test -- --nocapture
