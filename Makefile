# ABOUTME: Convenience wrapper around the cargo workflow for pingpong-rs.
# ABOUTME: Run `make help` to list available targets.

CARGO ?= cargo
# Config file used by `make run`.
CONFIG ?= pingpong.toml
# Extra flags for `make run`, e.g. `make run ARGS="--host 8.8.8.8 --animation globe"`.
ARGS ?=

.DEFAULT_GOAL := help

.PHONY: help build release run test test-verbose fmt fmt-check lint check ci clean install

help: ## List available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

build: ## Build the debug binary
	$(CARGO) build

release: ## Build the optimized release binary (target/release/pingpong-rs)
	$(CARGO) build --release

run: ## Run against $(CONFIG); pass extra flags via ARGS="..."
	$(CARGO) run -- --config $(CONFIG) $(ARGS)

test: ## Run the test suite
	$(CARGO) test

test-verbose: ## Run tests with captured output shown
	$(CARGO) test -- --nocapture

fmt: ## Format the code in place
	$(CARGO) fmt --all

fmt-check: ## Check formatting without modifying files
	$(CARGO) fmt --all -- --check

lint: ## Run clippy with warnings denied
	$(CARGO) clippy --all-targets --all-features -- -D warnings

check: ## Type-check without producing a binary
	$(CARGO) check

ci: fmt-check lint test ## Run the full gate: fmt-check + lint + test

clean: ## Remove build artifacts
	$(CARGO) clean

install: ## Install the binary into ~/.cargo/bin
	$(CARGO) install --path .
