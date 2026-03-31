.PHONY: all generate build test lint fmt clippy clean

all: generate build test lint

## Run the code generator (requires Node.js and codegen/)
generate:
	python3 codegen/generate.py --schema schemas/forum.json --output-dir src/generated/forum --module-name forum
	python3 codegen/generate.py --schema schemas/market.json --output-dir src/generated/market --module-name market

## Build the library
build:
	cargo build

## Run all tests
test:
	cargo test --all-features

## Run all lints
lint: fmt clippy

## Check formatting
fmt:
	cargo fmt --all -- --check

## Run clippy
clippy:
	cargo clippy --all-targets --all-features -- -D warnings

## Clean build artifacts
clean:
	cargo clean
