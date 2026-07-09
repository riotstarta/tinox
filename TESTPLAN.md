# Tinox — Testplan

Stand: 2026-07-06. Anlass: Beim jgrep/ygrep-Port wurden 16 Compiler-Bugs
gefunden (bugs.md), fast alle erst durch reale Programme — nicht durch die
bestehende Testsuite.

## Diagnose: Warum die Suite die Bugs nicht fängt

**Bestand:**
- ~1074 Rust-Unit-Tests (Lexer 246, Parser 274, Typecheck 304, Codegen 166, …)
- `tests/runtime_tests.sh`: 26 End-to-End-Tests per Bash, nicht automatisiert
- Kein CI, kein IR-Verifier-Gate, keine Regressionstests zu bugs.md,
  Dogfood-Projekte (jgrep-tinox, examples/) werden nirgends automatisch gebaut

**Strukturelle Lücken:**

1. **Codegen-Tests prüfen IR-Substrings, nicht Verhalten.**
   `assert!(ir.contains("fadd double"))` ist grün, auch wenn das Programm
   Müll rechnet. Kein einziger Cargo-Test führt kompilierte Programme aus.
2. **Bugs entstehen in Kontext-Übergängen, Tests testen Einzel-Features.**
   Die bugs.md-Liste ist monoton: match-gebundene Strings (1, 2, 5, 13, 14),
   let-Bindings von Listen/Maps (8, 15), Cross-Modul-Felder (6), Schleifen-
   Marker (15.4). Ursache ist immer dieselbe: der Codegen re-inferiert Typen
   ad hoc (Allowlists, funktionsflaches `local_types`, i64-Dispatch-Heuristik
   Map vs. String vs. Array). Ein Feature funktioniert als Literal, aber nicht
   als Listenelement / Match-Payload / Feld — genau diese Achse testet niemand.
3. **Stille Korruption statt lauter Fehler.** `tinox_map_len` auf einem
   String-Pointer liest Heap-Bytes; ob das auffällt, hängt vom Allokator ab
   (Bug 15: nur Zeilen mit 15–16 Zeichen verschwanden). Ohne Verifier/Checks
   werden solche Fehler erst Wochen später in Anwendungscode sichtbar.

---

## Phase 0 — Fundament (1–2 Tage, sofort)

### 0.1 `make check` als ein Einstiegspunkt + CI — ✅ erledigt (2026-07-09)
- Ein Target, das alles ausführt: `cargo test --release`, E2E-Suite (s. Phase 1),
  Dogfood-Builds (s. Phase 3). Lokal per Git-Hook (pre-push), optional
  GitHub Actions wenn das Repo remote liegt.
- Regel: **kein Compiler-Commit ohne grünes `make check`.** Der eine bereits
  rote Test (`test_process_produces_consumes`) wird gefixt oder als
  `#[ignore]` mit Begründung markiert — eine dauerrote Suite erzieht zum
  Wegschauen.
- **Stand:** `Makefile` mit `check` = Unit-Tests + `tests/runtime_tests.sh` +
  jgrep-Dogfood (Build + 170 Tests). Suite ist komplett grün: annotations-Test
  gefixt (@Produces/@Consumes mit String-Literal), hpack-Tests gefixt
  (`insert()` auf Listen, `fromCharCode`, Element-Typinferenz für
  `List<Class>`-Felder). Git-Hook/CI steht noch aus.

### 0.2 LLVM-IR-Verifier-Gate — ✅ erledigt (2026-07-09)
- Jedes generierte `.ll` in Tests und optional bei `tinox build` durch
  `opt -passes=verify` (bzw. `llvm-as -o /dev/null`) schicken.
- Fängt sofort die Klasse „ungültiges IR erst bei llc/opt sichtbar"
  (benannter void-Call beim ygrep-Port, `inttoptr i64` auf ptr aus Bug 15.4).
- Aufwand: ~1 Stunde. Höchstes Nutzen/Kosten-Verhältnis im ganzen Plan.
- **Stand:** `verify_ir()` in `crates/tinox/src/main.rs` läuft bei jedem
  `compile_ll_to_exe` (build/run/test, auch Debug-Modus, wo `opt` sonst ganz
  übersprungen wird). Meldet „internal compiler error: generated invalid
  LLVM IR" mit den ersten 20 Verifier-Zeilen. Bekannter offener Fund damit
  diagnostizierbar: Lambda-Block-Body auf `HttpServer.get` erzeugt
  `store ptr → i1`-Mismatch (vorher nur „opt failed").

---

## Phase 1 — Ausführende E2E-Suite (der Kern, ~1 Woche)

### 1.1 Golden-Test-Harness als Cargo-Test
- Verzeichnis `tests/e2e/*.tnx`; erwartete Ausgabe direkt in der Datei:
  ```tinox
  // expect-exit: 0
  // expect: 42
  // expect: hello
  fn main() -> Int64 { … }
  ```
- Ein Rust-Integrationstest (im `tinox`-Crate) iteriert das Verzeichnis:
  kompilieren → IR verifizieren → ausführen (Timeout) → stdout/stderr/Exit-Code
  vergleichen. Läuft unter `cargo test`, parallelisierbar, kein Bash.
- `tests/runtime_tests.sh` (26 Fälle) einmalig migrieren, dann löschen.

### 1.2 bugs.md → Regressionstests (16/16)
- Jeder Bug in bugs.md hat bereits eine Minimal-Repro — die wird 1:1 zu
  `tests/e2e/bug01_match_string_len.tnx` … `bug16_hash_string_literal.tnx`.
- Neue Regel: **ein Bug gilt erst als gefixt, wenn sein E2E-Test existiert.**
  bugs.md bekommt pro Eintrag einen Verweis auf die Testdatei.

### 1.3 Kontext-Matrix (der eigentliche Hebel gegen die Bug-Klasse)
Für jede Kern-Operation (`.len()`, `==`, `<`, `+`, `.substring()`,
`.contains()`, Indexzugriff, `for`-Iteration, `println`) denselben Wert durch
alle **Herkunfts-Kontexte** schleusen und identisches Verhalten verlangen:

| Kontext | Beispiel |
|---|---|
| Literal | `"hello".len()` |
| let-Binding | `let s = f(); s.len()` |
| var + Neuzuweisung | `var s = ""; s = f(); s.len()` |
| Funktionsparameter | `fn g(s: String) { s.len() }` |
| Funktionsrückgabe | `f().len()` |
| Listenelement | `xs[0].len()` — Liste aus Literal, aus Funktion, als Feld |
| Map-Value | `m.get(k).len()` |
| Klassenfeld | `this.s.len()`, `this.xs[i].len()` |
| Match-Payload | `match v { Str(s) => s.len() }` |
| Schleifenvariable | `for s in xs { s.len() }` — **und danach** gleichnamige Variable |
| Cross-Modul | dieselben Fälle über eine Modulgrenze (Bug 6) |

- Die Matrix wird **generiert** (kleines Rust- oder Shell-Skript erzeugt die
  .tnx-Dateien), nicht von Hand gepflegt: ~10 Operationen × ~12 Kontexte ×
  3 Typen (String, List, Map) ≈ 350 Fälle aus einem Generator von ~200 Zeilen.
- Hätte nachweislich Bugs 1, 2, 5, 6, 8, 13, 14, 15 gefangen.

### 1.4 Gedächtnis-/Grenzwert-Fälle
- Strings der Längen 0, 1, 7, 8, 15, 16, 17, 31, 32 (Heap-/Alignment-Grenzen;
  Bug 15 war nur bei 15–16 sichtbar), Umlaute/UTF-8, eingebettete `#`, `"`,
  `\n`, führende/abschließende Leerzeichen.
- Leere Listen/Maps, 1-elementig, verschachtelt ≥3 Ebenen, Listen von Maps
  von Listen. Enum-Payloads verschachtelt (Bugs 9, 10).

---

## Phase 2 — Generative Tests (danach, inkrementell)

### 2.1 Property-Tests auf Laufzeitebene
- Mit `proptest`: zufällige Strings/Listen generieren, daraus ein .tnx-Programm
  schreiben, das Identitäten prüft, z. B.
  `split(join(xs, sep), sep) == xs`, `s.substring(0, s.len()) == s`,
  `(a + b).len() == a.len() + b.len()` — erwartetes Ergebnis rechnet der
  Rust-Host, verglichen wird die Programmausgabe.
- ~5 generische Templates reichen; Shrinking liefert automatisch die
  Minimal-Repro für bugs.md.

### 2.2 Fuzzing des Frontends
- `cargo-fuzz` auf Lexer + Parser (nur Crash/Hang, kein Oracle nötig).
  Hätte Bug 16 (führendes `#` im String) vermutlich schnell gefunden.
- Später: Fuzzing bis Codegen mit IR-Verifier als Oracle.

### 2.3 Sanitizer-Lauf
- Wöchentlich (oder pre-release): E2E-Suite unter AddressSanitizer
  (runtime.c ist handgeschriebenes C — malloc ohne free überall) und
  einmal unter Valgrind. Ziel zunächst nur: keine **neuen** Fehler.

---

## Phase 3 — Dogfooding als CI-Gate (~½ Tag Setup)

jgrep-tinox hat mehr Bugs gefunden als alle Unit-Tests zusammen. Das
institutionalisieren:

- CI-Schritt „Dogfood": jgrep-tinox bauen + dessen 161 Tests ausführen,
  `examples/` (cli_test, rest_*, modules, stdlib) bauen und Smoke-Runs,
  `benchmarks/` kompilieren.
- Pfad konfigurierbar (`DOGFOOD_DIR`), lokal via Checkout nebenan.
- Compiler-Änderung, die Dogfood bricht → Commit blockiert. Genau die
  Situation aus dem ygrep-Port (Array:String-Fix brach den Evaluator) wäre
  damit vor dem Commit sichtbar gewesen, nicht danach.

---

## Phase 4 — Ursache statt Symptom (parallel, größer)

Testen lindert; die Bug-Quelle ist Architektur: **der Codegen re-inferiert
Typen, die der Typecheck schon kennt.** Funktionsflaches `local_types`,
Methoden-Allowlists (`split`/`keys`/…), Dispatch nach LLVM-Typ (`i64` →
Map-Heuristik) erzeugen systematisch die Bug-Klasse aus bugs.md.

- **Typisierter AST:** Typecheck annotiert jede Expression mit ihrem Typ;
  Codegen konsumiert nur noch (`expr.ty`), keine eigene Inferenz. Heuristiken
  (`is_map_dispatch`, `Array:String:elem`, Allowlists) schrittweise löschen —
  die Kontext-Matrix aus 1.3 ist dafür das Sicherheitsnetz.
- **Scoping:** `local_types`/`locals` blockscoped statt funktionsflach
  (Bug 15.4 war ein Scoping-Leck).
- **Debug-Modus mit Typ-Tags (optional):** `tinox build --checked` gibt
  Heap-Objekten ein Tag-Wort; Runtime-Funktionen prüfen es und brechen laut
  ab („map_len auf String bei x.tnx:12") statt still Müll zu lesen.

---

## Reihenfolge & Aufwand (realistisch)

| Schritt | Aufwand | Fängt |
|---|---|---|
| 0.2 IR-Verifier-Gate | ~1 h | ungültiges IR sofort |
| 0.1 make check (+ Hook) | ~2 h | „vergessene" Suiten |
| 1.1 E2E-Harness + Migration | 1 Tag | Verhaltensregressionen |
| 1.2 bugs.md-Regressionen | 1 Tag | Wiederkehr aller 16 Bugs |
| 1.3 Kontext-Matrix-Generator | 1–2 Tage | die dominante Bug-Klasse |
| 1.4 Grenzwert-Fälle | ½ Tag | Heap-Layout-Glückstreffer |
| 3 Dogfood-Gate | ½ Tag | Realwelt-Brüche vor Commit |
| 2.1 Property-Tests | 1–2 Tage | unbekannte Unbekannte |
| 2.2 Fuzzing | ½ Tag Setup | Frontend-Crashes |
| 2.3 Sanitizer | ½ Tag Setup | C-Runtime-UB |
| 4 Typisierter AST | Wochen, inkrementell | die Ursache |

Empfehlung: 0.1–1.4 und Phase 3 am Stück (≈ eine Woche), dann ist jede
weitere Compiler-Änderung abgesichert. Phase 2 danach nebenher, Phase 4 als
laufender Umbau mit der Matrix als Netz.

## Erfolgskriterien

- 16/16 bugs.md-Einträge haben einen ausgeführten Regressionstest.
- Jeder Test, der kompiliert, verifiziert auch sein IR.
- `make check` < 5 min lokal, blockierend vor jedem Commit.
- Neuer Bug ⇒ zuerst E2E-Repro-Test (rot), dann Fix (grün), dann bugs.md-Eintrag.
- Dogfood (jgrep/ygrep 161 Tests + examples) ist Teil von `make check`.
