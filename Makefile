.PHONY: all build-frontend build build-go test test-contract lint lint-rust vet clean

# Build the frontend into web/dist (required by Rust build.rs / rust-embed).
build-frontend:
	cd web && npm run build

# Default: frontend + Rust release binary → bin/local_agent
build: build-frontend
	cargo build --release
	mkdir -p bin
	cp -f target/release/local_agent bin/local_agent

# Legacy Go oracle binary (optional).
build-go: build-frontend
	rm -rf internal/server/dist/*
	cp -r web/dist/* internal/server/dist/
	go build -o bin/app ./cmd/app

# Run Rust unit tests (quiet).
test:
	cargo test -q --all-targets

# Run the black-box contract differential runner against the Rust backend
# (default). Use CONTRACT_BACKEND=go to compare against the legacy Go oracle.
# The `contract` feature gate keeps this out of `cargo test --all-targets`.
test-contract:
	CONTRACT_BACKEND=$${CONTRACT_BACKEND:-rust} cargo test --test contract_runner --features contract -- --nocapture

# Run golangci-lint (cross-platform: Windows, macOS, Linux).
lint:
	golangci-lint run

# Run cargo clippy (Rust). Deny levels come from [lints] in Cargo.toml;
# -D warnings matches CI so local runs catch the same bar.
lint-rust:
	cargo clippy --all-targets -- -D warnings

# Run go vet (legacy Go tree).
vet:
	go vet ./...

# Clean build artifacts.
clean:
	rm -rf bin/
	rm -rf web/dist/
	rm -rf internal/server/dist/*
	touch internal/server/dist/.gitkeep
	cargo clean
