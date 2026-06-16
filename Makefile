.PHONY: dev build bundle open test dev-bundle ci fmt lint

ci: lint test ## Run lint + tests

fmt:
	pnpm fmt
	cd src-tauri && cargo fmt --all

lint: ## Check formatting and clippy
	pnpm lint
	pnpm fmt:check
	cd src-tauri && cargo fmt --all -- --check
	cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings

dev:
	pnpm tauri dev

build:
	pnpm tauri build -b app

bundle:
	pnpm install
	pnpm tauri build -b app

open: bundle
	open src-tauri/target/release/bundle/macos/planeai.app

test:
	pnpm test
	cd src-tauri && cargo test --workspace

test-e2e: build
	./tests/e2e_session_persistence.sh

dev-bundle:
	$(eval BRANCH := $(shell git branch --show-current | sed 's|/|-|g'))
	$(eval SUFFIX := $(if $(filter main,$(BRANCH)),dev,$(if $(BRANCH),$(shell echo $(BRANCH) | sed 's/-/ /g' | awk '{for(i=1;i<=NF;i++) printf substr($$i,1,1); printf int(rand()*10)}'),dev)))
	@# Swap identifier, productName, and binary name for isolated dev build
	sed -i '' 's/"productName": "planeai"/"productName": "planeai-$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' 's/"identifier": "ca.nicolegros.planeai"/"identifier": "ca.nicolegros.planeai.$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' '/^\[package\]/,/^\[/{s/^name = "planeai"/name = "planeai-$(SUFFIX)"/;}' src-tauri/Cargo.toml
	sed -i '' '/^\[\[bin\]\]/,/^\[/{s/^name = "planeai"/name = "planeai-$(SUFFIX)"/;}' src-tauri/Cargo.toml
	pnpm tauri build -b app || (git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml && exit 1)
	git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml
	@echo "\n✅ Dev bundle ready: src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app"
	open -n src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app
