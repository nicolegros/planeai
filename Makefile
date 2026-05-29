.PHONY: dev build bundle open test dev-bundle

dev:
	pnpm tauri dev

build:
	pnpm tauri build -b app

bundle:
	pnpm tauri build -b app

open: bundle
	open src-tauri/target/release/bundle/macos/planeai.app

test:
	pnpm test
	cd src-tauri && cargo test

dev-bundle:
	$(eval BRANCH := $(shell git branch --show-current | sed 's|/|-|g'))
	$(eval SUFFIX := $(if $(filter main,$(BRANCH)),dev,$(if $(BRANCH),$(BRANCH),dev)))
	@# Swap identifier, productName, and binary name for isolated dev build
	sed -i '' 's/"productName": "planeai"/"productName": "planeai-$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' 's/"identifier": "ca.nicolegros.planeai"/"identifier": "ca.nicolegros.planeai.$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' 's/^name = "planeai"/name = "planeai-$(SUFFIX)"/' src-tauri/Cargo.toml
	pnpm tauri build -b app || (git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml && exit 1)
	git checkout -- src-tauri/tauri.conf.json src-tauri/Cargo.toml
	@echo "\n✅ Dev bundle ready: src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app"
	open -n src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app
