# Ein Einstiegspunkt für alles (TESTPLAN Phase 0.1):
# Regel: kein Compiler-Commit ohne grünes `make check`.

.PHONY: check test e2e dogfood

check: test e2e dogfood

# Rust-Unit-Tests (Lexer, Parser, Typecheck, Codegen, …)
test:
	cargo test --release

# End-to-End: Golden-Tests (tests/e2e/*.tnx) über den Cargo-Harness
e2e:
	cargo test --release -p tinox --test e2e

# Dogfood: jgrep/ygrep bauen und deren Testsuiten laufen lassen (wenn ausgecheckt)
dogfood:
	@if [ -d ../jgrep-tinox ]; then \
		cargo build --release && \
		cd ../jgrep-tinox && \
		PATH=$(CURDIR)/target/release:$$PATH bash build.sh && \
		for t in tests/*_test.tnx; do \
			PATH=$(CURDIR)/target/release:$$PATH tinox test "$$t" || exit 1; \
		done; \
	else \
		echo "dogfood: ../jgrep-tinox nicht gefunden — übersprungen"; \
	fi
