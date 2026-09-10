# SYNTH — Build, Test, and CI Automation
.DEFAULT_GOAL := help

SHELL := /bin/bash
CARGO ?= cargo

## Colors for formatted help output
COLOR_RESET   := \033[0m
COLOR_BOLD    := \033[1m
COLOR_CYAN    := \033[36m
COLOR_GREEN   := \033[32m
COLOR_YELLOW  := \033[33m

.PHONY: help
help: ## Display this help message
	@echo -e "$(COLOR_BOLD)SYNTH Developer Automation$(COLOR_RESET)"
	@echo -e "Usage: $(COLOR_CYAN)make$(COLOR_RESET) $(COLOR_YELLOW)<target>$(COLOR_RESET)"
	@echo ""
	@echo -e "$(COLOR_BOLD)Available Targets:$(COLOR_RESET)"
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  $(COLOR_CYAN)%-18s$(COLOR_RESET) %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.PHONY: all
all: check test lint ## Run full verification suite (check, test, lint)

.PHONY: build
build: ## Build all workspace crates in release mode
	@echo -e "$(COLOR_GREEN)==> Building release workspace...$(COLOR_RESET)"
	$(CARGO) build --workspace --release

.PHONY: build-debug
build-debug: ## Build all workspace crates in debug mode
	@echo -e "$(COLOR_GREEN)==> Building debug workspace...$(COLOR_RESET)"
	$(CARGO) build --workspace

.PHONY: check
check: ## Typecheck all workspace crates
	@echo -e "$(COLOR_GREEN)==> Running cargo check across workspace...$(COLOR_RESET)"
	$(CARGO) check --workspace

.PHONY: test
test: ## Run the entire workspace test suite
	@echo -e "$(COLOR_GREEN)==> Running workspace test suite...$(COLOR_RESET)"
	$(CARGO) test --workspace

.PHONY: test-registry
test-registry: ## Run component registry integrity and vector search tests
	@echo -e "$(COLOR_GREEN)==> Verifying component registry...$(COLOR_RESET)"
	$(CARGO) test -p synth-registry

.PHONY: lint
lint: ## Run Clippy lints across all workspace targets
	@echo -e "$(COLOR_GREEN)==> Running cargo clippy...$(COLOR_RESET)"
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: fmt
fmt: ## Check code formatting
	@echo -e "$(COLOR_GREEN)==> Checking formatting with rustfmt...$(COLOR_RESET)"
	$(CARGO) fmt --all --check

.PHONY: fmt-fix
fmt-fix: ## Automatically fix code formatting
	@echo -e "$(COLOR_GREEN)==> Formatting Rust code...$(COLOR_RESET)"
	$(CARGO) fmt --all

.PHONY: deny
deny: ## Run cargo-deny checks (licenses, bans, advisories)
	@echo -e "$(COLOR_GREEN)==> Running cargo deny check...$(COLOR_RESET)"
	$(CARGO) deny check

.PHONY: clean
clean: ## Clean build target directory
	@echo -e "$(COLOR_YELLOW)==> Cleaning target directory...$(COLOR_RESET)"
	$(CARGO) clean

.PHONY: run-cli
run-cli: ## Run the synth CLI (pass ARGS="...")
	$(CARGO) run -p synth-cli -- $(ARGS)

.PHONY: run-web
run-web: ## Launch the local synth-web browser preview server
	$(CARGO) run -p synth-web

.PHONY: verify
verify: fmt check lint test test-registry ## Full CI pre-flight verification gate
	@echo -e "$(COLOR_GREEN)$(COLOR_BOLD)✔ All pre-flight checks passed!$(COLOR_RESET)"
