# Ein Einstiegspunkt für alles (TESTPLAN Phase 0.1):
# Regel: kein Compiler-Commit ohne grünes `make check`.
# `make install-hooks` aktiviert das pre-push-Gate.

DOGFOOD_DIR ?= ../jgrep-tinox
export DOGFOOD_DIR

.PHONY: check test e2e dogfood install-hooks asan checked

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

# Sanitizer-Lauf (TESTPLAN 2.3): E2E-Suite mit AddressSanitizer-Runtime.
# Plain malloc statt Boehm-GC (-DTINOX_NO_GC), damit ASan jede Allokation
# sieht; Leaks sind dabei Absicht (detect_leaks=0), Ziel sind Overflows/UAF.
# Nicht Teil von `make check` — wöchentlich/vor Releases laufen lassen.
asan:
	cargo build --release
	TINOX_CFLAGS="-fsanitize=address -g -DTINOX_NO_GC" \
	ASAN_OPTIONS="detect_leaks=0" \
	cargo test --release -p tinox --test e2e --test boundary

# Checked-Lauf (TESTPLAN Phase 4): E2E- + Grenzwert-Suite mit
# Heap-Kind-Registry (-DTINOX_CHECKED, siehe `tinox build --checked`).
# Array-/Map-Runtime-Funktionen prüfen ihre Pointer — Dispatch-Bugs
# brechen laut ab. Grün = keine False Positives im gesamten Testbestand.
checked:
	cargo build --release
	TINOX_CFLAGS="-DTINOX_CHECKED" \
	cargo test --release -p tinox --test e2e --test boundary

# Git-Hooks aktivieren (pre-push führt `make check` aus)
install-hooks:
	git config core.hooksPath .githooks
	@echo "core.hooksPath = .githooks gesetzt (pre-push: make check)"
