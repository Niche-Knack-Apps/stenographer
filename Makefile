APP       := stenographer
VERSION   := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
RELEASE   ?= ../_shared/releases/$(APP)

# Binary names
DAEMON    := stenographerd
CLI       := steno
ALIAS     := sten

# Detect host platform
UNAME_S   := $(shell uname -s)
UNAME_M   := $(shell uname -m)

ifeq ($(UNAME_S),Linux)
  HOST_PLATFORM := linux
  ifeq ($(UNAME_M),aarch64)
    MUSL_TARGET := aarch64-unknown-linux-musl
    GNU_TARGET  := aarch64-unknown-linux-gnu
  else
    MUSL_TARGET := x86_64-unknown-linux-musl
    GNU_TARGET  := x86_64-unknown-linux-gnu
  endif
  HAS_MUSL := $(shell which musl-gcc 2>/dev/null && echo yes)
  ifeq ($(HAS_MUSL),yes)
    HOST_TARGET := $(MUSL_TARGET)
  else
    HOST_TARGET := $(GNU_TARGET)
  endif
endif
ifeq ($(UNAME_S),Darwin)
  HOST_PLATFORM := mac
  HOST_TARGET   := $(UNAME_M)-apple-darwin
endif
ifeq ($(OS),Windows_NT)
  HOST_PLATFORM := windows
  HOST_TARGET   := x86_64-pc-windows-msvc
endif

EXT       := $(if $(filter windows,$(HOST_PLATFORM)),.exe,)
DIST      := dist
CARGO     := cargo

DATA_DIR  := $(HOME)/.local/share/com.niche-knack.stenographer
HOOKS_DIR := $(DATA_DIR)/hooks
SVC_DIR   := $(HOME)/.config/systemd/user

.PHONY: all build release dev dev-stop check test clippy fmt verify \
        build-static install uninstall clean dist-clean help

all: build

# ─── Development ──────────────────────────────────────────────

dev: ## Fast debug build + start daemon
	$(CARGO) build
	@echo "Starting $(DAEMON) in background..."
	@target/debug/$(DAEMON) &
	@sleep 0.3
	@echo "$(DAEMON) running (pid $$(cat $${XDG_RUNTIME_DIR:-/tmp}/stenographer/steno.pid 2>/dev/null || echo '?'))"

dev-stop: ## Stop the dev daemon
	@PID_FILE="$${XDG_RUNTIME_DIR:-/tmp}/stenographer/steno.pid"; \
	if [ -f "$$PID_FILE" ]; then \
		PID=$$(cat "$$PID_FILE"); \
		kill "$$PID" 2>/dev/null && echo "Stopped $(DAEMON) (pid $$PID)" || echo "$(DAEMON) not running"; \
	else \
		echo "No PID file found — $(DAEMON) not running"; \
	fi

check: ## Type-check without building
	$(CARGO) check

test: ## Run all tests
	$(CARGO) test

clippy: ## Run clippy linter
	$(CARGO) clippy -- -D warnings

fmt: ## Check formatting
	$(CARGO) fmt -- --check

verify: clippy test fmt ## Run all checks (clippy + test + fmt)

# ─── Build ────────────────────────────────────────────────────

build: ## Release build → dist/
	@echo "Building $(APP) v$(VERSION) for $(HOST_TARGET)..."
	$(CARGO) build --release --target $(HOST_TARGET)
	@mkdir -p $(DIST)
	cp target/$(HOST_TARGET)/release/$(DAEMON)$(EXT) $(DIST)/$(DAEMON)$(EXT)
	cp target/$(HOST_TARGET)/release/$(CLI)$(EXT) $(DIST)/$(CLI)$(EXT)
	@echo ""
	@ls -lh $(DIST)/$(DAEMON)$(EXT) $(DIST)/$(CLI)$(EXT)

build-static: ## Static musl build (requires musl-tools)
	@command -v musl-gcc >/dev/null 2>&1 || \
		{ echo "Error: musl-gcc not found. Install musl-tools: sudo apt install musl-tools"; exit 1; }
	@echo "Building static $(APP) v$(VERSION) for $(MUSL_TARGET)..."
	$(CARGO) build --release --target $(MUSL_TARGET)
	@mkdir -p $(DIST)
	cp target/$(MUSL_TARGET)/release/$(DAEMON) $(DIST)/$(DAEMON)
	cp target/$(MUSL_TARGET)/release/$(CLI) $(DIST)/$(CLI)

# ─── Release ──────────────────────────────────────────────────

release: build ## Release build → _shared/releases/stenographer/{platform}
	@mkdir -p $(RELEASE)/$(HOST_PLATFORM)
	cp $(DIST)/$(DAEMON)$(EXT) $(RELEASE)/$(HOST_PLATFORM)/$(DAEMON)-$(VERSION)-$(HOST_TARGET)$(EXT)
	cp $(DIST)/$(CLI)$(EXT) $(RELEASE)/$(HOST_PLATFORM)/$(CLI)-$(VERSION)-$(HOST_TARGET)$(EXT)
	@echo "Released to $(RELEASE)/$(HOST_PLATFORM)/"
	@ls -lh $(RELEASE)/$(HOST_PLATFORM)/

# ─── Install ──────────────────────────────────────────────────

install: build ## Build, install, and print the source command for the current shell
	$(DIST)/$(CLI)$(EXT) install
	@echo ""
	@echo "  (make runs in a subprocess and cannot source your shell directly)"
	@echo "  Copy the command above and run it, or paste:  source ~/.zshrc"

# ─── Uninstall ────────────────────────────────────────────────

uninstall: ## Remove binaries, service (preserves data)
	-$(HOME)/.local/bin/$(CLI) hook uninstall 2>/dev/null || true
	-systemctl --user disable --now stenographer.service 2>/dev/null || true
	rm -f $(SVC_DIR)/stenographer.service
	-systemctl --user daemon-reload 2>/dev/null || true
	rm -f $(HOME)/.local/bin/$(DAEMON) $(HOME)/.local/bin/$(CLI) $(HOME)/.local/bin/$(ALIAS)
	rm -rf $(HOOKS_DIR)
	@echo "Uninstalled. Data preserved at $(DATA_DIR)/ — remove manually if desired."

# ─── Clean ────────────────────────────────────────────────────

clean: ## Remove build artifacts
	$(CARGO) clean
	rm -rf $(DIST)

dist-clean: clean ## Remove dist/ and release outputs
	rm -rf $(RELEASE)/$(HOST_PLATFORM)

# ─── Help ─────────────────────────────────────────────────────

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'
