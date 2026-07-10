# Ein Einstiegspunkt für alles (TESTPLAN Phase 0.1):
# Regel: kein Compiler-Commit ohne grünes `make check`.
# `make install-hooks` aktiviert das pre-push-Gate.

DOGFOOD_DIR ?= ../jgrep-tinox
export DOGFOOD_DIR

.PHONY: check test e2e dogfood install-hooks

check: test e2e dogfood

# Rust-Unit-Tests (Lexer, Parser, Typecheck, Codegen, …)
test:
	cargo test --release

# End-to-End: Golden-Tests (tests/e2e/*.tnx) + generierte Kontext-Matrix + Grenzwerte
e2e:
	cargo test --release -p tinox --test e2e --test matrix --test boundary

# Dogfood: examples/ + benchmarks/ bauen, jgrep/ygrep bauen und testen
# (jgrep-Checkout via DOGFOOD_DIR konfigurierbar)
dogfood:
	cargo build --release
	bash scripts/dogfood.sh

# Git-Hooks aktivieren (pre-push führt `make check` aus)
install-hooks:
	git config core.hooksPath .githooks
	@echo "core.hooksPath = .githooks gesetzt (pre-push: make check)"
