.PHONY: dev build bundle open test test-plugin-fixture dev-bundle ci fmt lint docs install sidecars local-plugin-fixture

# Dummy updater signing key for local builds (not used in CI releases)
ifeq ($(OS),Windows_NT)
FIXTURE_ARCH := $(if $(filter ARM64,$(PROCESSOR_ARCHITECTURE)),arm64,x64)
FIXTURE_PLATFORM := windows-$(FIXTURE_ARCH)
else
FIXTURE_OS := $(if $(filter Darwin,$(shell uname -s)),macos,linux)
FIXTURE_ARCH := $(if $(filter arm64 aarch64,$(shell uname -m)),arm64,x64)
FIXTURE_PLATFORM := $(FIXTURE_OS)-$(FIXTURE_ARCH)
endif
FIXTURE_BINARY := planeai-plugin-fixture$(if $(filter Windows_NT,$(OS)),.exe)

DUMMY_SIGNING_KEY := dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5QnlHWnBkWklHc0lISUlrbDg0L29zSHR0L1NQQWovcHlsbVNRaDd3TXhxQUFBQkFBQUFBQUFBQUFBQUlBQUFBQXhTY3gvZW82clBCNUhCdWtoTkZNZEhJaVRUMkh0OVZsUzVESDdhU1JjR2ZwT3l4NlhTUEtvVnlpVjVsSFAwUDQ5aWF4QlVCUWJuRlFULy9DR1JQWC95dk04QTJNbGgvVTdRTHdiUmxrRHh1clQrWWgzdUY5bTZsQzl1OVFoYWgzVlRXK3gvajVrRzQ9Cg==
SIGNING_ENV := TAURI_SIGNING_PRIVATE_KEY=$${TAURI_SIGNING_PRIVATE_KEY:-$(DUMMY_SIGNING_KEY)} TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}

ci: install lint test ## Run lint + tests

install:
	pnpm install

fmt:
	pnpm fmt
	cd src-tauri && cargo fmt --all

lint: ## Check formatting and clippy
	pnpm lint
	pnpm exec svelte-check
	pnpm fmt:check
	cd src-tauri && cargo fmt --all -- --check
	cd src-tauri && JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo clippy --workspace --all-targets --all-features -- -D warnings

dev: sidecars
	cd src-tauri && cargo build -p planeai-plugin-jira
	RUST_LOG=planeai=debug pnpm exec tauri dev

dogfood: ## Run Iced workflow shell (ensures planeai-pty + durable logs)
	cd src-tauri && \
	PLANEAI_DAEMON_PTY_CORE=planeai-pty \
	PLANEAI_SESSION_LOG_DIR="$${HOME}/.local/share/planeai/session-logs" \
	cargo run --release -p planeai-iced-spike --bin planeai-iced -- \
		--planeai-workflow \
		--backend iced-alacritty

sidecars:
	cd src-tauri && ./scripts/ensure-sidecars.sh

build: sidecars
	$(SIGNING_ENV) pnpm exec tauri build -b app

bundle: install sidecars
	$(SIGNING_ENV) pnpm exec tauri build -b app
	@echo "$(CURDIR)/src-tauri/target/release/bundle/macos/planeai.app" | pbcopy
	@echo "✅ Bundle path copied to clipboard"

local-plugin-fixture:
	cd src-tauri && cargo build -p planeai-plugin-fixture
	mkdir -p src-tauri/plugins/local-fixture/bin/$(FIXTURE_PLATFORM)
	cp src-tauri/target/debug/$(FIXTURE_BINARY) src-tauri/plugins/local-fixture/bin/$(FIXTURE_PLATFORM)/$(FIXTURE_BINARY)
	chmod +x src-tauri/plugins/local-fixture/bin/$(FIXTURE_PLATFORM)/$(FIXTURE_BINARY)

open: bundle
	open src-tauri/target/release/bundle/macos/planeai.app

test: test-plugin-fixture
	pnpm test
	cd src-tauri && env -u PLANEAI_DAEMON_PTY_CORE -u PLANEAI_SESSION_LOG_DIR JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo test --workspace

test-plugin-fixture: local-plugin-fixture
	cd src-tauri && JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo run -p planeai-cli-bin -- plugin test --package plugins/local-fixture
	cd src-tauri && JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo run -p planeai-cli-bin -- plugin test --package plugins/local-fixture --scenario plugins/local-fixture/scenarios/persist-settings.jsonl
	cd src-tauri && JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo run -p planeai-cli-bin -- plugin test --package plugins/local-fixture --scenario plugins/local-fixture/scenarios/state-environment.jsonl
	cd src-tauri && JIRA_CLIENT_ID=$${JIRA_CLIENT_ID:-dummy} JIRA_CLIENT_SECRET=$${JIRA_CLIENT_SECRET:-dummy} cargo run -p planeai-cli-bin -- plugin test --package plugins/local-fixture --scenario plugins/local-fixture/scenarios/cancellation.jsonl

test-e2e: build
	./tests/e2e_session_persistence.sh

dev-bundle: sidecars
	$(eval BRANCH := $(shell git branch --show-current | sed 's|/|-|g'))
	$(eval SUFFIX := $(if $(filter main,$(BRANCH)),dev,$(if $(BRANCH),$(shell echo $(BRANCH) | sed 's/-/ /g' | awk '{for(i=1;i<=NF;i++) printf substr($$i,1,1); printf int(rand()*10)}'),dev)))
	@# Swap identifier, productName, and binary name for isolated dev build
	sed -i '' 's/"productName": "planeai"/"productName": "planeai-$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' 's/"identifier": "ca.nicolegros.planeai"/"identifier": "ca.nicolegros.planeai.$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' '/^\[package\]/,/^\[/{s/^name = "planeai"/name = "planeai-$(SUFFIX)"/;}' src-tauri/Cargo.toml
	sed -i '' '/^\[\[bin\]\]/,/^\[/{s/^name = "planeai"/name = "planeai-$(SUFFIX)"/;}' src-tauri/Cargo.toml
	$(SIGNING_ENV) pnpm exec tauri build -b app || (git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml && exit 1)
	git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml
	@echo "\n✅ Dev bundle ready: src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app"
	open -n src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app

docs: ## Run docs site locally
	cd docs && pnpm dev
