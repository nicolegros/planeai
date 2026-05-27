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
	@# Swap identifier and productName for isolated dev build
	sed -i '' 's/"productName": "planeai"/"productName": "planeai-dev"/' src-tauri/tauri.conf.json
	sed -i '' 's/"identifier": "ca.nicolegros.planeai"/"identifier": "ca.nicolegros.planeai.dev"/' src-tauri/tauri.conf.json
	pnpm tauri build -b app || (git checkout -- src-tauri/tauri.conf.json && exit 1)
	git checkout -- src-tauri/tauri.conf.json
	@echo "\n✅ Dev bundle ready: src-tauri/target/release/bundle/macos/planeai-dev.app"
	open src-tauri/target/release/bundle/macos/planeai-dev.app
