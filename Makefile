.DEFAULT_GOAL := help
SHELL := /bin/bash
COMPOSE := docker compose
# The browser container runs as you, so what it writes (node_modules, screenshots) is yours to delete.
E2E_USER := --user $$(id -u):$$(id -g) -e HOME=/tmp

.PHONY: help env up down reset logs fmt lint test check smoke e2e transcripts sabotage screenshots

help: ## list the targets
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-12s %s\n", $$1, $$2}'

env: ## write .env with a fresh random ledger key (never overwrites an existing one)
	@if [ -f .env ]; then echo ".env already exists, leaving it alone"; else \
	  echo "LEASH_LEDGER_KEY=$$(openssl rand -hex 32)" > .env; echo "wrote .env with a fresh random ledger key"; fi

up: env ## build and start the server (http://localhost:8090)
	$(COMPOSE) up --build --wait -d
	@echo; echo "  site + lab + API:  http://localhost:$${LEASH_PORT:-8090}   (press 'Enter the lab')"; echo "  stop with:         make down"

down: ## stop and keep the ledger
	$(COMPOSE) down

reset: ## stop and delete the ledger and the runtime copy of the documents
	$(COMPOSE) down -v

logs: ## follow the logs
	$(COMPOSE) logs -f app

fmt: ## format the Rust code
	cargo fmt --all

lint: ## rustfmt check and clippy with every warning an error
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy -p leash-bridge --features vsock --all-targets -- -D warnings

test: ## every unit, integration and doc test
	cargo test --workspace

check: lint test ## everything the CI checks except the browser tests

smoke: ## twelve checks against the running server (make up first)
	scripts/smoke.sh http://localhost:$${LEASH_PORT:-8090}

e2e: env ## browser tests (Playwright + axe) against the Compose container
	@mkdir -p docs/img  # Docker would create a missing bind-mount source as root, and then you could not write to docs/
	$(COMPOSE) up --build --wait -d
	$(COMPOSE) --profile e2e run --rm $(E2E_USER) e2e

transcripts: ## re-record the guided tour on the page from the running server
	node scripts/record-transcripts.mjs

sabotage: ## break each defence on purpose and confirm a test notices
	scripts/sabotage.sh

screenshots: env ## re-take the README screenshots (docs/img) from the running container
	@mkdir -p docs/img
	$(COMPOSE) up --build --wait -d
	$(COMPOSE) --profile e2e run --rm $(E2E_USER) -e SHOTS=1 e2e bash -lc "npm ci --no-audit --no-fund && npx playwright test shots"
