.PHONY: setup submodules ghostty project build run test clean

setup: submodules ghostty project ## Full setup from scratch

submodules: ## Pull/update git submodules
	git submodule update --init --recursive

ghostty: ## Build GhosttyKit xcframework (requires zig@0.15)
	./scripts/build-ghostty.sh

project: ## Regenerate Xcode project from project.yml
	xcodegen generate

build: ## Build the app
	xcodebuild build -project PlaneAI.xcodeproj -scheme PlaneAI -destination 'platform=macOS'

run: build ## Build and run the app
	open "$$(find ~/Library/Developer/Xcode/DerivedData/PlaneAI-*/Build/Products/Debug -name 'PlaneAI.app' -type d | head -1)"

test: ## Run PlaneAICore tests
	cd Packages/PlaneAICore && swift test

clean: ## Clean build artifacts
	xcodebuild clean -project PlaneAI.xcodeproj -scheme PlaneAI
	rm -rf Packages/PlaneAICore/.build

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-12s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
