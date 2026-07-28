.PHONY: all build-frontend build test test-contract lint lint-rust lint-frontend check clean mockagent setup

# Verify/install prerequisites (tools, versions, frontend deps).
setup:
	./scripts/setup.sh

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
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

# Run eslint on the frontend (web/). Matches `npm run lint`.
lint-frontend:
	cd web && npm run lint

# Unified lint: Rust (fmt + clippy) + frontend (eslint).
lint: lint-rust lint-frontend

# Unified pre-push gate: full CI parity in one command.
#   1. Rust fmt + clippy (-D warnings)
#   2. Rust unit tests (--all-targets)
#   3. Frontend eslint + tsc/vite build (typecheck + SPA embed)
#   4. Contract suite (HTTP/WS black-box)
# Slowest target (~1-2 min); use `make lint` for a fast style/correctness pass.
check: lint build-frontend test test-contract

# Auto-fix formatting and linting for Rust.
fix-rust:
	cargo fmt --all
	cargo clippy --all-targets --fix --allow-dirty --allow-staged --allow-no-vcs

# Auto-fix formatting and linting for the frontend.
fix-frontend:
	cd web && npm run lint -- --fix

# Unified auto-fix: Rust + frontend.
fix: fix-rust fix-frontend

# Quiet check: Auto-fixes code, then runs tests quietly. Fails loudly on error.
qcheck: fix
	@echo "Running tests quietly..."
	@cargo test -q --all-targets > /dev/null 2>&1 || (echo "Rust tests failed" && exit 1)
	@cd web && npm run build --quiet > /dev/null 2>&1 || (echo "Frontend build failed" && exit 1)
	@CONTRACT_BACKEND=rust cargo test -q --test contract_runner --features contract > /dev/null 2>&1 || (echo "Contract tests failed" && exit 1)
	@echo "All tests passed successfully!"

# Build the Rust mock ACP agent used by Rust spike/ACP tests.
mockagent:
	cargo build --bin mockagent
	mkdir -p bin
	cp -f target/debug/mockagent bin/mockagent

# Clean build artifacts.
clean:
	rm -rf bin/
	rm -rf web/dist/
	cargo clean
