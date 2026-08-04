# Ein Einstiegspunkt für alles (TESTPLAN Phase 0.1):
# Regel: kein Compiler-Commit ohne grünes `make check`.
# `make install-hooks` aktiviert das pre-push-Gate.

DOGFOOD_DIR ?= ../jgrep-tinox
export DOGFOOD_DIR

.PHONY: check test e2e dogfood install-hooks asan checked clippy fuzz

check: clippy test e2e dogfood

# Lint-Gate: 0 Warnings über den ganzen Workspace (Fehler + Tests).
# Bewusste Ausnahmen stehen als #[allow(...)] mit Begründung im Code.
clippy:
	cargo clippy --release --workspace --all-targets -- -D warnings

# Rust-Unit-Tests (Lexer, Parser, Typecheck, Codegen, …)
test:
	cargo test --release

# End-to-End: Golden-Tests (tests/e2e/*.tnx) + generierte Kontext-Matrix
# + Grenzwerte + Stdlib-Smoke-Gate (jedes tinox-core-Modul einmal benutzen)
e2e:
	cargo test --release -p tinox --test e2e --test matrix --test boundary --test stdlib_smoke

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

# Fuzz regression check (issue #111, see fuzz/README.md for the full
# rationale/architecture): builds every libFuzzer harness in fuzz/*/ and
# runs each briefly against its checked-in seed corpus. -fork=N so the
# harness's own well-documented per-worker OOM (every target builds
# runtime.c with -DTINOX_NO_GC, same as `make asan` — nothing is ever
# freed, so a single worker's RSS grows unbounded) gets recycled instead
# of stopping the whole campaign. FUZZ_SECONDS is per-target, not total —
# override for a longer local run, e.g. `make fuzz FUZZ_SECONDS=300`.
#
# Found while wiring this up: libFuzzer's -fork driver still exits
# non-zero (observed: 71) whenever ANY worker hit -rss_limit_mb during the
# run, even with -ignore_ooms=1 (verified: doesn't change the exit code,
# only governs whether the coordinator keeps scheduling new jobs after an
# OOM). For these harnesses that is the EXPECTED steady state, not a
# finding — e.g. fuzz/zip calls zipEntryCount/zipEntryName/zipEntrySize,
# each re-reading + re-parsing the whole input from scratch, so a single
# input burns 3 allocations per exec; at 100k+ exec/s that alone reaches
# -rss_limit_mb=2048 well inside a 60s run under -DTINOX_NO_GC. The same
# never-freed heap also makes libFuzzer misreport slow-unit-* "hangs": as
# a worker's heap grows into the hundreds of MB/GB over the run, glibc
# malloc's own bookkeeping overhead grows with it, so late-run inputs take
# longer in wall-clock terms than early-run ones even though they're not
# algorithmically slower -- verified by replaying several slow-unit-*
# artifacts standalone in a *fresh* process (tiny <500-byte inputs, each
# parses in ~0.1-0.17s, nothing like the outlier that got them flagged).
# So: trust the artifacts, not the raw exit code. -fork writes one file
# per stop-condition to -artifact_prefix, named by kind (oom-*,
# slow-unit-*, crash-*, timeout-*, leak-*) -- oom-*/slow-unit-* are the
# two harmless cases above; crash-*/timeout-*/leak-* are real
# libFuzzer/ASan findings. So each target gets its own empty
# fuzz/$t/artifacts/ dir per run, and only a crash-*/timeout-*/leak-* file
# there (or a non-zero exit with NO artifact at all to explain it) fails
# `make fuzz` -- a run that exits non-zero with only oom-*/slow-unit-*
# files is logged and treated as a pass.
# Not part of `make check` (like asan/checked) — weekly/pre-release via
# .github/workflows/deep-checks.yml.
FUZZ_SECONDS ?= 60
fuzz:
	cargo build --release -p tinox
	@set -e; \
	for t in json zip hpack amqp091 amqp10; do \
		echo "=== fuzz/$$t ==="; \
		bash fuzz/$$t/build.sh; \
		mkdir -p fuzz/$$t/corpus; \
		rm -rf fuzz/$$t/artifacts; mkdir -p fuzz/$$t/artifacts; \
		status=0; \
		ASAN_OPTIONS=detect_leaks=0 fuzz/$$t/$${t}_fuzzer \
			-fork=4 -max_total_time=$(FUZZ_SECONDS) -rss_limit_mb=2048 -detect_leaks=0 \
			-ignore_ooms=1 -artifact_prefix=fuzz/$$t/artifacts/ \
			fuzz/$$t/corpus/ fuzz/$$t/seeds/ || status=$$?; \
		real_findings=$$(find fuzz/$$t/artifacts -type f ! -name 'oom-*' ! -name 'slow-unit-*' 2>/dev/null); \
		harmless=$$(find fuzz/$$t/artifacts -type f \( -name 'oom-*' -o -name 'slow-unit-*' \) 2>/dev/null | wc -l); \
		if [ -n "$$real_findings" ]; then \
			echo "fuzz/$$t: REAL finding(s), not just OOM/slow-unit recycling:"; \
			echo "$$real_findings"; \
			exit 1; \
		elif [ "$$status" != 0 ] && [ "$$harmless" = 0 ]; then \
			echo "fuzz/$$t: exited $$status with no artifact to explain it"; \
			exit 1; \
		elif [ "$$status" != 0 ]; then \
			echo "fuzz/$$t: exit $$status but only $$harmless harmless oom-*/slow-unit-* artifact(s) (expected under -DTINOX_NO_GC) -- treating as pass"; \
		fi; \
		rm -rf fuzz/$$t/artifacts; \
	done

# Git-Hooks aktivieren (pre-push führt `make check` aus)
install-hooks:
	git config core.hooksPath .githooks
	@echo "core.hooksPath = .githooks gesetzt (pre-push: make check)"
