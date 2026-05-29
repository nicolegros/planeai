.PHONY: dev build open test dev-bundle

dev:
	pnpm tauri dev

build:
	pnpm tauri build -b app

open: build
	open src-tauri/target/release/bundle/macos/planeai.app

test:
	pnpm test
	cd src-tauri && cargo test

dev-bundle:
	$(eval BRANCH := $(shell git branch --show-current | sed 's|/|-|g'))
	$(eval SUFFIX := $(if $(filter main,$(BRANCH)),dev,$(if $(BRANCH),$(BRANCH),dev)))
	@# Swap identifier and productName for isolated dev build
	sed -i '' 's/"productName": "planeai"/"productName": "planeai-$(SUFFIX)"/' src-tauri/tauri.conf.json
	sed -i '' 's/"identifier": "ca.nicolegros.planeai"/"identifier": "ca.nicolegros.planeai.$(SUFFIX)"/' src-tauri/tauri.conf.json
	pnpm tauri build -b app || (git checkout -- src-tauri/tauri.conf.json && exit 1)
	git checkout -- src-tauri/tauri.conf.json
	@echo "\n✅ Dev bundle ready: src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app"
	open src-tauri/target/release/bundle/macos/planeai-$(SUFFIX).app
