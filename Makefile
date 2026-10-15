# Boiler Room — common tasks. Run `make help`.
export PATH := $(HOME)/.cargo/bin:$(PATH)

HAS_UI  := $(wildcard package.json)
HAS_APP := $(wildcard src-tauri/Cargo.toml)

.DEFAULT_GOAL := help
.PHONY: help setup fmt lint test check build dev app clean

##@ Help
help: ## Show this help
	@awk 'BEGIN { FS = ":.*## "; \
	      printf "\n  \033[1;33mBOILER ROOM\033[0m  \033[2m- make <target>\033[0m\n" } \
	  /^##@/ { printf "\n  \033[1;36m%s\033[0m\n", substr($$0, 5); next } \
	  /^[a-z-]+:.*## / { printf "    \033[32m%-8s\033[0m %s\n", $$1, $$2 } \
	  END { print "" }' $(MAKEFILE_LIST)

##@ Setup
setup: ## Install frontend dependencies (Rust toolchain via rustup, see README)
ifdef HAS_UI
	pnpm install
else
	@echo "no frontend yet; nothing to install"
endif

##@ Quality
fmt: ## Format Rust code
	cargo fmt --all

lint: ## Clippy (warnings are errors) + format check
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
ifdef HAS_UI
	pnpm lint
endif

test: ## Run all tests
	cargo test --workspace
ifdef HAS_UI
	pnpm test
endif

check: lint test ## Everything CI should run

##@ Build & run
build: ## Release build of the Rust workspace (core only until the app exists)
	cargo build --workspace --release

dev: ## Launch the desktop app in dev mode
ifdef HAS_APP
	pnpm tauri dev
else
	@echo "desktop app not scaffolded yet (milestone 0.4)"
endif

app: ## Build installable desktop bundles for this OS
ifdef HAS_APP
	pnpm tauri build
else
	@echo "desktop app not scaffolded yet (milestone 0.4)"
endif

##@ Maintenance
clean: ## Remove build output
	cargo clean
	rm -rf dist
