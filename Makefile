.PHONY: dev build open test

dev:
	pnpm tauri dev

build:
	pnpm tauri build -b app

open: build
	open src-tauri/target/release/bundle/macos/planeai.app

test:
	pnpm test
	cd src-tauri && cargo test
