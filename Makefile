.PHONY: all build-frontend build test test-contract lint-rust clean mockagent

# Build the frontend into web/dist (required by Rust build.rs / rust-embed).
build-frontend:
	cd web && npm run build

# Default: frontend + Rust release binary → bin/local_agent
build: build-frontend
	cargo build --release
	mkdir -p bin
	cp -f target/release/local_agent bin/local_agent

# Run Rust unit tests (quiet).
test:
	cargo test -q --all-targets

# Black-box contract suite against the Rust backend.
# The `contract` feature gate keeps this out of `cargo test --all-targets`.
test-contract:
	CONTRACT_BACKEND=rust cargo test --test contract_runner --features contract -- --nocapture

# Run cargo clippy (Rust). Deny levels come from [lints] in Cargo.toml;
# -D warnings matches CI so local runs catch the same bar.
lint-rust:
	cargo clippy --all-targets -- -D warnings

# Build the Go mock ACP agent used by Rust spike/ACP tests.
mockagent:
	go build -o bin/mockagent ./cmd/mockagent

# Clean build artifacts.
clean:
	rm -rf bin/
	rm -rf web/dist/
	cargo clean
