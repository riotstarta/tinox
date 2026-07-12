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
  `List<Class>`-Felder). Pre-push-Hook seit Phase 3; CI seit 2026-07-12:
  `.github/workflows/check.yml` läuft volles `make check` bei Push/PR,
  jgrep-tinox wird als Geschwister-Checkout geklont (öffentliches Repo).

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

### 1.1 Golden-Test-Harness als Cargo-Test — ✅ erledigt (2026-07-10)
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
- **Stand:** `crates/tinox/tests/e2e.rs` (4 parallele Shards, Direktiven
  `expect`/`expect-exit`/`expect-contains`/`args`/`db`/`mode: test`);
  alle 38 Bash-Fälle migriert, runtime_tests.sh gelöscht, `make e2e` läuft
  über Cargo. Beim Migrieren gefunden und gefixt: `tinox build/run`
  exitete 0 bei Compilerfehlern.

### 1.2 bugs.md → Regressionstests — ✅ erledigt (2026-07-10)
- Jeder Bug in bugs.md hat bereits eine Minimal-Repro — die wird 1:1 zu
  `tests/e2e/bug01_match_string_len.tnx` … `bug16_hash_string_literal.tnx`.
- Neue Regel: **ein Bug gilt erst als gefixt, wenn sein E2E-Test existiert.**
  bugs.md bekommt pro Eintrag einen Verweis auf die Testdatei.
- **Stand:** `tests/e2e/bug01…bug17` (Bug 11 war kein Tinox-Bug). Die Repros
  fanden sofort vier weitere Fehler, alle gefixt: Rückgabeklassen von
  Top-Level-Funktionen wurden nicht registriert (Bug-6-Klasse:
  `let r = modFn(); r.feld` las Offset 0), `List<Float64>`-Elemente kamen
  als i64-Bitmuster zurück, kein trailing comma in List-Literalen,
  Build-Exit-Code (s. 1.1).

### 1.3 Kontext-Matrix — ✅ erledigt (2026-07-10)
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
- **Stand:** `crates/tinox/tests/matrix.rs` generiert die Fälle zur Laufzeit
  (String, List<Int64>, List<String> × 14 Kontexte, alle Ops pro Datei);
  KNOWN_FAILURES-Liste erzwingt Pflege bei Fixes. Der erste Lauf fand
  12 fehlschlagende Fälle (Element-Typisierung verschachtelter Listen,
  for-in über Felder/Literale, ops auf Literalen/Call-Ausdrücken,
  Match-Payload-Listen) — alle geschlossen durch ein einheitliches
  Marker-System: `container_marker` (AST-Typ → "Array:String",
  "Array:Array:…", "List:C", "Map") + `elem_marker` (eine Ebene strippen),
  konsumiert von Index-Codegen, Methoden-Dispatch, for-in, let/var,
  Match-Payload-Bindung und method_ret_class-Registrierung.
  KNOWN_FAILURES ist leer. Nachtrag 2026-07-11: Map-Value-Kontext
  (`m.get(k)` für alle Typen) plus Typen Float64/List<Float64> ergänzt
  (damit auch Float-Schleifenvariablen und Cross-Modul-Floats) — fand
  20 Fälle, geschlossen durch Map-Value-Marker ("Map:String"/"Map:Float"/
  "Map:<marker>") und double-Slots für Float-Schleifenvariablen.
  Außerdem Maps als Subjekt-Typen (Map<String,Int64/String> als
  @{…}-Literal durch alle 15 Kontexte) — dabei MapLiteral-let-Bindings
  gefixt, die den Annotations-Marker verwarfen (Map<String,String>-Werte
  waren Pointer-Müll). Matrix jetzt 7 Typen × 15 Kontexte.

### 1.4 Gedächtnis-/Grenzwert-Fälle — ✅ erledigt (2026-07-10)
- Strings der Längen 0, 1, 7, 8, 15, 16, 17, 31, 32 (Heap-/Alignment-Grenzen;
  Bug 15 war nur bei 15–16 sichtbar), Umlaute/UTF-8, eingebettete `#`, `"`,
  `\n`, führende/abschließende Leerzeichen.
- Leere Listen/Maps, 1-elementig, verschachtelt ≥3 Ebenen, Listen von Maps
  von Listen. Enum-Payloads verschachtelt (Bugs 9, 10).
- **Stand:** `crates/tinox/tests/boundary.rs` (generiert, Ground truth vom
  Rust-Host): String-Längen-Sweep 0…32 durch Literal/Fn-Return/split/Concat,
  Sonderzeichen (#, Quotes, \n, \t, Umlaute, Leerzeichen), leere/
  1-elementige Container, List<List<List<Int64>>>, List<Map>, Map mit
  List-Values. Dabei gefunden und gefixt: `>>>` in
  `List<List<List<…>>>` wurde als Shift-Token gelext (Parser splittet
  jetzt), und der let/var-Annotations-Fallback markierte `Array<T>`
  pauschal als Array:String (jetzt container_marker). Enum-Payload-
  Verschachtelung deckt bug09/bug10 ab.

---

## Phase 2 — Generative Tests (danach, inkrementell)

### 2.1 Property-Tests auf Laufzeitebene — ✅ erledigt (2026-07-10)
- Mit `proptest`: zufällige Strings/Listen generieren, daraus ein .tnx-Programm
  schreiben, das Identitäten prüft, z. B.
  `split(join(xs, sep), sep) == xs`, `s.substring(0, s.len()) == s`,
  `(a + b).len() == a.len() + b.len()` — erwartetes Ergebnis rechnet der
  Rust-Host, verglichen wird die Programmausgabe.
- ~5 generische Templates reichen; Shrinking liefert automatisch die
  Minimal-Repro für bugs.md.
- **Stand:** `crates/tinox/tests/properties.rs` — 8 Properties × 12
  Instanzen gegen das Rust-Orakel (join/split-Roundtrip, substring,
  concat-Länge+Inhalt, sort, reverse-Involution, push/pop-Modell,
  contains/indexOf, replace). Deterministisch geseedet statt proptest
  (Compile-Zeit dominiert; `TINOX_PROP_SEED` variiert, Seed steht im
  Fehlerreport). Über 4 Seeds grün.

### 2.2 Fuzzing des Frontends — ✅ Basis erledigt (2026-07-10)
- `cargo-fuzz` auf Lexer + Parser (nur Crash/Hang, kein Oracle nötig).
  Hätte Bug 16 (führendes `#` im String) vermutlich schnell gefunden.
- Später: Fuzzing bis Codegen mit IR-Verifier als Oracle.
- **Stand:** `crates/tinox-parser/tests/robustness.rs` (Pseudo-Fuzzing ohne
  cargo-fuzz, deterministisch geseedet, Watchdog mit 10-s-Timeout pro
  Input): 1500 Zufalls-Inputs, 1500 Mutationen eines validen Programms,
  alle EOF-Präfixe. **Fand beim ersten Lauf einen echten Hänger:**
  `catch e: S[ring` — der Array-Typ-Skip-Loop in parse_type drehte bei
  EOF ohne `]` endlos (gefixt). Echtes cargo-fuzz mit Corpus bleibt
  als Ausbau offen.

### 2.3 Sanitizer-Lauf — ✅ erledigt (2026-07-10)
- Wöchentlich (oder pre-release): E2E-Suite unter AddressSanitizer
  (runtime.c ist handgeschriebenes C — malloc ohne free überall) und
  einmal unter Valgrind. Ziel zunächst nur: keine **neuen** Fehler.
- **Stand:** `make asan` — E2E- + Grenzwert-Suite mit ASan-instrumentierter
  Runtime. Boehm-GC ist für ASan unsichtbar, daher `-DTINOX_NO_GC`
  (plain calloc, Leaks Absicht, detect_leaks=0); `TINOX_CFLAGS` wird vom
  Compiler an beide cc-Aufrufe durchgereicht. Erster Lauf: sauber.
  Bewusst nicht Teil von `make check` (Laufzeit); wöchentlich/pre-release.

---

## Phase 3 — Dogfooding als CI-Gate — ✅ erledigt (2026-07-10)

jgrep-tinox hat mehr Bugs gefunden als alle Unit-Tests zusammen. Das
institutionalisieren:

- CI-Schritt „Dogfood": jgrep-tinox bauen + dessen 161 Tests ausführen,
  `examples/` (cli_test, rest_*, modules, stdlib) bauen und Smoke-Runs,
  `benchmarks/` kompilieren.
- Pfad konfigurierbar (`DOGFOOD_DIR`), lokal via Checkout nebenan.
- Compiler-Änderung, die Dogfood bricht → Commit blockiert. Genau die
  Situation aus dem ygrep-Port (Array:String-Fix brach den Evaluator) wäre
  damit vor dem Commit sichtbar gewesen, nicht danach.
- **Stand:** `scripts/dogfood.sh` (via `make dogfood`, Teil von
  `make check`): 6 examples bauen + 3 Smoke-Runs mit Soll-Ausgabe,
  3 benchmarks kompilieren, jgrep bauen + 5 Suiten (170 Tests);
  `DOGFOOD_DIR` konfigurierbar. Pre-push-Hook: `.githooks/pre-push`
  führt `make check` aus, aktiviert per `make install-hooks`
  (core.hooksPath). Bekannt kaputte Beispiele (vorbestehend, im Skript
  dokumentiert): examples.tnx (Int32/Int64-Mix), interface_extends +
  mini_http (Library ohne main), rest_with_mini (@Json_deserialize
  fehlt), modules/multi_import (alte `::`-Importsyntax).

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
  - **Schritt 1 ✅ (2026-07-11):** `ValueType::Array(elem)`/`Map(val)`
    strukturell statt typgelöscht; Literale/Index/Schleifenvariablen
    inferieren elementgenau, `types_compatible`/`lub` rekursiv
    (Any-Element = Wildcard, keine falschen Ablehnungen — Suite blieb
    grün). Fängt jetzt `let xs: List<String> = [1,2]` u. Ä. zur
    Compile-Zeit; Fehlermeldungen elementgenau via `display()`
    (`to_string()` bleibt Dispatch-Key!).
  - **Schritt 2 ✅ (2026-07-11):** Typleitung Typecheck→Codegen steht.
    `Spanned` trägt NodeIds (`assign_node_ids` nach Import-Resolve;
    ID 0 = synthetisch/unbekannt), der Typecheck füllt `expr_types`
    (NodeId → ValueType), Export als Marker-Tabelle (`expr_markers()`,
    ValueType→Marker-Sprache). Codegen konsultiert sie als **Fallback**
    (nie Override): `infer_struct_type`, let/var-Bindings, for-in-
    Iterables, Methoden-Dispatch. Schließt Fälle ohne Heuristik-Arm
    (z. B. `match` als Wert → `tests/e2e/typed_ast_expr_table.tnx`).
  - **Schritt 3 ✅ (2026-07-11):** Erste Heuristiken gelöscht, Quelle
    präzisiert. Builtin-Signaturen elementtypisiert (String_split,
    Map_keys, regexFindAll/Split, dirList, processArgs → List<String>);
    receiver-abhängige Ergebnistypen im MethodCall-Arm (get/values auf
    Map<_,V>, first/last/find/min/max/pop/sort/… auf List<E>) — nach
    check_call, Validierung bleibt. Damit gelöscht: split/keys-Allowlist
    (let+var) und dirList/processArgs-Sonderfall im Codegen. Dabei
    gefunden: `var` ignorierte seine Typ-Annotation komplett (ungeprüft,
    Wert-Typ gewann) — jetzt Let-Regel; und die Tabelle muss letzte
    Präzedenz sein (Annotation > lokale Inferenz > Tabelle), sonst
    überstimmt typgelöschtes Map::new die Annotation. Verbleibende
    Heuristiken (i64-Dispatch-Raten, array_only_methods,
    Array:String:elem) brauchen Tabellen-Anschluss weiterer
    Expression-Positionen — nächster Schritt.
  - **Schritt 4 ✅ (2026-07-11):** `Array:String:elem` abgeschafft —
    String-Schleifenvariablen sind echte i8*-Slots (Muster wie
    Match-Payloads/Float-Loops), der Cast-bei-Nutzung-Sonderfall im
    Ident-Codegen ist gelöscht; Marker ist schlicht "String".
    Map-Index-Zuweisung (`m[k] = v`, auch `this.m[k]`) an
    infer_struct_type/Tabelle angeschlossen. Verbleibende Heuristiken
    array_only_methods + i64-Map-Methodenraten sind jetzt reine
    Fallbacks für typecheck-Any-Werte (jq-artiger dynamischer Code) —
    löschen erst, wenn Any-Verbreitung im Typecheck weiter sinkt.
  - **Schritt 5 ✅ (2026-07-11):** Match-Payload-Bindungen typisiert.
    Typecheck registriert Payload-Typen pro Variante
    (enum_variant_payloads, "Enum::Variant"), bind_pattern_vars bindet
    mit Scrutinee-/Payload-Typ statt Any (nacktes Pattern: Variantenname
    steht in enum_name!). Fand Bug 18: Klassen-Payloads waren im Codegen
    "Other"/ungetypt — u.name im match-Arm las Offset 0 (die id, als
    Zahl). Fix: FieldAccess-Fallback auf die Tabelle. Matrix um
    Klasseninstanzen als Subjekt-Typ erweitert (TypeSpec-prelude,
    8 Typen × 15 Kontexte; cross_module für Klassen ausgenommen).
- **Scoping:** `local_types`/`locals` blockscoped statt funktionsflach
  (Bug 15.4 war ein Scoping-Leck). — Geprüft 2026-07-11: Typecheck
  verbietet verschachteltes Shadowing komplett („duplicate definition"),
  Geschwister-Scope-Redeklaration funktioniert; kein beobachtbares Leck
  konstruierbar. Niedrige Priorität.
- **Debug-Modus mit Typ-Tags (optional):** `tinox build --checked` gibt
  Heap-Objekten ein Tag-Wort; Runtime-Funktionen prüfen es und brechen laut
  ab („map_len auf String bei x.tnx:12") statt still Müll zu lesen.
  — ✅ umgesetzt 2026-07-12 als Heap-Kind-Registry statt Tag-Wort (kein
  ABI-Unterschied: Seitentabelle, Strings/Literale bleiben nackte char*).
  Array-/Map-Konstruktoren (tinox_array_new, tinox_map_create,
  make_static_map, json_obj_map_create) registrieren, alle Array-/Map-
  Runtime-Funktionen prüfen und abort()en mit klarer Meldung.
  `tinox build/run --checked` (via TINOX_CFLAGS -DTINOX_CHECKED),
  `make checked` = E2E+Grenzwerte im Checked-Modus. Validiert: E2E,
  Matrix und kompletter Dogfood (jgrep, Any-lastig) ohne False
  Positives; ein Arena-Map-Konstruktor im JSON-Parser wurde dabei als
  dritter Registrierungspunkt gefunden. Demo: map_get über ungetypte
  Lambda-Param auf einem String → sofortiger Abbruch mit
  „map_get auf unregistriert (String/Objekt?)-Pointer" statt stillem
  Absturz.

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
