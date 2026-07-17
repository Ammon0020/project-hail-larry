.PHONY: all build-frontend build-backend build test test-contract lint lint-rust vet clean

# Build the frontend and copy it into the Go embed directory.
build-frontend:
	cd web && npm run build
	rm -rf internal/server/dist/*
	cp -r web/dist/* internal/server/dist/

# Build the Go binary with embedded frontend.
build-backend:
	go build -o bin/local-agent ./cmd/app

# Build everything: frontend + backend.
build: build-frontend build-backend

# Run all tests.
test:
	go test ./...
	cd web && npm run build

# Run the black-box contract differential runner against the Go backend.
# Use CONTRACT_BACKEND=rust to test against the Rust backend instead.
# The `contract` feature gate keeps this out of `cargo test --all-targets`.
test-contract:
	cargo test --test contract_runner --features contract -- --nocapture

# Run golangci-lint (cross-platform: Windows, macOS, Linux).
lint:
	golangci-lint run

# Run cargo clippy (Rust). Deny levels come from [lints] in Cargo.toml;
# -D warnings matches CI so local runs catch the same bar.
lint-rust:
	cargo clippy --all-targets -- -D warnings

# Run go vet.
vet:
	go vet ./...

# Clean build artifacts.
clean:
	rm -rf bin/
	rm -rf web/dist/
	rm -rf internal/server/dist/*
	touch internal/server/dist/.gitkeep
