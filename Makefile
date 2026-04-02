# depends-on-rs

.PHONY: help fmt check test ci

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "%-12s %s\n", $$1, $$2}'

fmt: ## Format the Rust code
	cargo fmt

check: ## Run cargo check
	cargo check

test: ## Run cargo test
	cargo test

ci: fmt check test ## Run the local CI sequence

.DEFAULT_GOAL := help
