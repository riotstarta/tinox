# Tinox Compiler Bugs

Discovered while porting jq to Tinox (`/home/tg7c49/git/jgrep-tinox`).
Each bug has a minimal reproduction and a description of expected vs. actual behavior.
Fix them in order — later bugs may depend on earlier fixes being in place.

---

## Bug 1 — `s.len()` auf match-gebundenen Strings gibt ASCII-Code zurück

**Status: GEFIXT (2026-07-05)** — Match-Bindings verwenden jetzt den echten LLVM-Typ aus der Enum-Deklaration (`bind_match_payload` + `enum_variant_payloads`-Pre-Pass in codegen.rs). String-Payloads werden als `i8*` gebunden, damit greift der normale String-Dispatch.

**Regressionstest:** `tests/e2e/bug01_match_string_len.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match v {
    Str(s) => return s.len();
    _ => return 0;
}
```
Gibt den ASCII-Code des letzten Zeichens von `s` zurück, nicht die Länge.

**Ursache:**
Match-gebundene Variablen aus Enum-Varianten werden als `i64` in lokalen Slots gespeichert
(via `alloca i64`, `store i64 payload`). Wenn dann `s.len()` aufgerufen wird, erkennt der
String-Dispatch-Code (`obj_ty == "i64" && is_str_method("len")`) den Aufruf als String-Methode
und macht ein `inttoptr i64 → i8*`. Danach wird `@tinox_string_length(i8* s)` aufgerufen.
Das `i64`-Payload eines `Str`-Variants ist aber nicht der String-Pointer direkt, sondern der
gesamte Payload-Slot des Enum-Structs `[disc, payload]`, der selbst ein `ptrtoint` der `i8*`
ist. Das `inttoptr` liefert also den richtigen String-Pointer — aber `@tinox_string_length`
scheint auf diesen Wert falsch zu operieren.

*Genauere Diagnose nötig:* möglicherweise liegt das Problem darin, dass `s` nach dem
match-binding noch den rohen i64-Payload enthält (den ptrtoint-Wert des String-Pointers),
nicht den String-Pointer selbst, und `inttoptr` daraus eine falsche Adresse macht.

**Workaround in jgrep:**
```tinox
fn strLen(s: String) -> Int64 { return s.len(); }
// Verwendung: strLen(sv) statt sv.len()
```

**Erwartetes Verhalten:**
`s.len()` soll die Anzahl der Zeichen im String zurückgeben.

---

## Bug 2 — `==` auf match-gebundenen Strings gibt immer `false`

**Status: GEFIXT (2026-07-05)** — Durch denselben Fix wie Bug 1: String-Payloads sind jetzt `i8*`, `==` ruft `@tinox_string_equals` auf.

**Regressionstest:** `tests/e2e/bug02_match_string_eq.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match a {
    Str(sv) =>
        match b {
            Str(bs) => return sv == bs;
            _ => return false;
        }
    _ => return false;
}
```
Gibt immer `false`, auch wenn `sv` und `bs` denselben String enthalten.

**Ursache:**
Beide match-gebundenen Variablen `sv` und `bs` sind `i64` (ptrtoint der String-Pointer).
Der `==`-Operator für `i64`-Werte führt einen Pointer-Vergleich (`icmp eq i64`) durch,
nicht einen String-Inhalts-Vergleich. Zwei verschiedene `String`-Allokationen mit gleichem
Inhalt haben unterschiedliche Adressen → `false`.

**Workaround in jgrep:**
```tinox
fn strEq(a: String, b: String) -> Bool { return a == b; }
// Verwendung: strEq(sv, bs) statt sv == bs
```

**Erwartetes Verhalten:**
`sv == bs` soll `@tinox_string_equals` aufrufen (Inhalts-Vergleich).

---

## Bug 3 — `<`/`>` Operatoren für String-Parameter sind Compilerfehler

**Status: GEFIXT (2026-07-05)** — Typecheck erlaubt jetzt `<`/`<=`/`>`/`>=` für String/String; Codegen emittiert `@tinox_string_compare` (neu in runtime.c) + icmp auf dem Ergebnis.

**Regressionstest:** `tests/e2e/bug03_string_ordering.tnx`

**Datei:** `crates/tinox-typecheck/src/lib.rs` oder `crates/tinox-parser/`

**Problem:**
```tinox
fn compare(a: String, b: String) -> Int64 {
    if a < b { return -1; }  // Compilerfehler!
    if a > b { return 1; }   // Compilerfehler!
    return 0;
}
```
Der Typecheck lehnt `<`/`>` für `String`-Typen ab.

**Ursache:**
Der Typecheck erlaubt `<`/`>` nur für numerische Typen, nicht für `String`.
Für Strings wäre ein lexikographischer Vergleich via `@tinox_string_compare` sinnvoll.

**Workaround in jgrep:**
Zeichenweiser Vergleich via `fn strCmp(a: String, b: String) -> Int64` (implementiert in `json_value.tnx`).

**Erwartetes Verhalten:**
`a < b` für Strings soll lexikographischen Vergleich durchführen (wie in den meisten Sprachen).

---

## Bug 4 — `Float64` ist komplett kaputt

**Status: GEFIXT (2026-07-05)** — Basis-Float-Support (Literale, Arithmetik, Parameter, Vergleiche, `toString`) funktionierte bereits über die bestehenden bitcast-Pfade. Zwei verbliebene Fehler behoben: (a) `list.push(1.5)` emittierte ungültiges `i64 1.5` (double→i64-bitcast im push-Codegen ergänzt), (b) match-gebundene `Float64`-Payloads wurden als i64-Bitmuster behandelt (payload_kind "Float" → Bindung als `double` via bitcast). Hinweis: `toString()` formatiert 4.0 als "4" (wie jq).

**Regressionstest:** `tests/e2e/bug04_float64.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
let x: Float64 = 3.14;
print(x.toString());  // Gibt z.B. "4614253070214989087" aus (IEEE-754 Bitmuster)
let y = x + 1.0;      // Rechnet auf Bitmuster-Integers
```

**Ursache:**
Float64-Werte werden intern als `i64` (Bitmuster) gespeichert und behandelt.
`toString()` konvertiert das `i64`-Bitmuster direkt zu Decimal statt IEEE-754 decode.
Arithmetik-Operatoren operieren auf den Bitmuster-Integers statt auf Float-Werten.

**Workaround in jgrep:**
Floats werden als `String` im `JsonValue::Float(String)`-Variant gespeichert.
Arithmetik über String-Parsing (`strToFloat`, `floatToStr` via C-Bridge).

**Erwartetes Verhalten:**
`Float64` soll IEEE-754 double-precision sein. Arithmetik korrekt, `toString()` gibt
Dezimaldarstellung aus (z.B. `"3.14"`).

---

## Bug 5 — Match-gebundene List/Map-Referenzen sind korrupt

**Status: GEFIXT (2026-07-05)** — Durch denselben Fix wie Bug 1: List-Payloads werden als `i64*` gebunden (Array-Dispatch + Iteration funktionieren), Map-Payloads als `i8*` mit `local_types = "Map"` (Map-Dispatch greift).

**Regressionstest:** `tests/e2e/bug05_match_list_map.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match v {
    Array(a) => {
        let n = a.len();    // Gibt Speicheradresse zurück, nicht Länge
        for x in a { ... } // Iteriert einmal mit dem Original-Array-Wert
    }
    Object(o) => {
        // o.len() gibt Speicheradresse zurück
        // o.get(key) funktioniert aber (nach Codegen-Fix für Map-Dispatch, siehe Bug-Liste)
    }
}
```

**Ursache:**
Das `i64`-Payload eines `Array`- oder `Object`-Variants ist ein `ptrtoint` des
List/Map-Pointers. Nach dem match-binding enthält die Variable diesen `i64`-Wert.
Wenn dann `a.len()` aufgerufen wird und der Dispatch nicht korrekt über Map-Dispatch
geht, wird der `i64`-Wert direkt als Länge interpretiert — was die Speicheradresse ist.

Für `for x in a`: Der Iterator-Codegen läuft `a.len()`-mal, aber `a` ist `i64`, der Iterator
bekommt den `Array`-Wrapper statt die Elemente.

**Workaround in jgrep:**
```tinox
fn copyList(a: List<JsonValue>) -> List<JsonValue> { return a; }
fn copyMap(o: Map<String, JsonValue>) -> Map<String, JsonValue> { return o; }
// Verwendung:
match v {
    Array(a) => { let arr = copyList(a); for x in arr { ... } }
    Object(o) => { let obj = copyMap(o); let n = obj.len(); }
}
```

**Erwartetes Verhalten:**
Match-gebundene List/Map-Variablen sollen wie normale lokale List/Map-Variablen funktionieren.

---

## Bug 6 — Cross-Modul Struct-Feld-Korruption

**Status: GEFIXT (2026-07-05)** — Zwei Match-Codegen-Fehler: (a) No-Arg-Variant-Arme emittierten `icmp eq i64` auf einem `ptr`-typisierten Match-Subjekt (ungültige IR → "opt failed"), (b) Payload-Arme dereferenzierten ptr-typisierte Subjekte ohne 65535-Pointer-Guard (Segfault bei No-Arg-Werten). Alle drei Enum-Match-Pfade zu einem vereinheitlicht: Subjekt wird auf i64 normalisiert, Guard greift immer. Verifiziert mit Cross-Modul-Struct mit Str/Null/Integer-Enum-Feld.

**Regressionstest:** `tests/e2e/bug06_cross_module_struct.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
// Modul A definiert:
class Result {
    var value: JsonValue;  // oder anderes Enum-Feld
}
fn getResult() -> Result { ... }

// Modul B importiert und nutzt:
let r = getResult();
print(r.value);  // Feld ist null oder falscher Variant
```

Struct mit `JsonValue`/Enum-Feldern, die über Modulgrenze zurückgegeben werden:
Das Enum-Feld ist korrupt (null oder falscher Variant).

**Ursache:**
Vermutlich ABI-Problem: Structs werden über Modulgrenzen als `i64*` (Pointer) übergeben,
aber die Feld-Offsets oder die Interpretation des Enum-Payloads weicht ab.

**Workaround in jgrep:**
`List<JsonValue>` statt Structs über Modulgrenzen zurückgeben.
Innerhalb desselben Moduls funktioniert alles korrekt.

**Erwartetes Verhalten:**
Struct-Felder sollen über Modulsgrenzen korrekt erhalten bleiben.

---

## Bug 7 — `dirList()` Elemente-Zugriff segfaultet

**Status: GEFIXT (2026-07-05)** — `dirList` gab laut Codegen `i8*` zurück (Fallback: Return-Typ = Typ des ersten Arguments) und wurde als String behandelt. Jetzt expliziter Builtin-Case im Call-Codegen (`i64*`-Rückgabe) + `local_types = "Array:String"` bei Let-Bindung. len/Index/Iteration funktionieren.

**Regressionstest:** `tests/e2e/bug07_dirlist_elements.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs` oder `runtime/runtime.c`

**Problem:**
```tinox
let entries = dirList("/some/path");
let n = entries.len();  // funktioniert korrekt
let first = entries[0]; // Segfault
for e in entries { ... } // Segfault
```

**Ursache:**
`dirList()` gibt eine `List<String>` zurück, bei der `len()` korrekt ist,
aber Index-Zugriff und `for`-Iteration segfaulten. Wahrscheinlich ähnlich wie Bug 5:
die zurückgegebene Liste ist ein korrupter Wrapper.

**Workaround in jgrep:**
```tinox
fn copyStringList(a: List<String>) -> List<String> { return a; }
let entries = copyStringList(dirList(path));
```

**Erwartetes Verhalten:**
`dirList()` soll eine vollständig funktionale `List<String>` zurückgeben.

---

## Bug 8 — `let ks = map.keys()` typisiert Elemente nicht als String

**Status: GEFIXT (2026-07-05)** — Die Let-Binding-Inferenz kannte `Array:String` nur für `.split()`-Methodenaufrufe; `.keys()` fehlte. Iteration über das Ergebnis druckte Pointer-Werte statt Strings. Fix: `method == "keys"` in beiden MethodCall-Inferenz-Armen (let/var) ergänzt.

**Regressionstest:** `tests/e2e/bug08_map_keys_typing.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match v {
    Obj(o) => {
        let ks = o.keys();
        for k in ks { print(k); }  // druckt Zahlen (Pointer) statt Keys
    }
}
```

**Entdeckt beim:** Rückbau der wrapObj/objKeys-Workarounds in jgrep — genau diese Helfer hatten das Problem kaschiert (fnc mit deklariertem Rückgabetyp `List<String>` lieferte korrekte Typinfo).

---

## Bug 9 — Verschachtelte Filter-Enum-Payloads werden korrupt

**Status: GEFIXT (2026-07-05)** — Nicht mehr reproduzierbar nach dem Match-Binding-Fix (Bug 1/2/5/13/14). Die Korruption lag im Auslesen der Payloads per Match, nicht im Speichern. Verifiziert mit rekursivem `Pipe(Filter, Filter)`-Test.

**Regressionstest:** `tests/e2e/bug09_nested_enum_payloads.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
// Filter-Enum:
enum Filter {
    Pipe(Filter, Filter),
    Reduce(Filter, String, Filter, Filter),
    // ...
}
// Wenn Pipe als Payload eines Reduce gespeichert wird:
let f = Filter::Reduce(gen, var, init, Filter::Pipe(a, b));
// Beim Auslesen: äußeres Tag korrekt, innere Payload-Felder von Pipe sind korrupt
```

**Ursache:**
Wenn ein Enum-Variant als Payload-Feld eines anderen Enum-Variants gespeichert wird
(tief verschachtelt), werden die inneren Payload-Felder beim Auslesen per match korrupt.
Das äußere Discriminator-Tag stimmt, aber die `getelementptr i64, ptr, i64 1+`-Offsets
greifen auf falsche Speicherbereiche.

**Workaround in jgrep:**
Reduce/Foreach als `FunctionCall("__reduce__", args)` mit `List<Filter>` als args kodieren,
statt als eigene Enum-Varianten mit Filter-Payloads.

**Erwartetes Verhalten:**
Verschachtelte Enum-Payloads sollen korrekt gespeichert und ausgelesen werden können.

---

## Bug 10 — List-Literal mit gemischten Enum-Werten ist korrupt

**Status: GEFIXT (2026-07-05)** — Nicht mehr reproduzierbar nach dem Match-Binding-Fix. Verifiziert mit gemischtem List-Literal (`[Identity, LiteralStr("x"), FieldAccess("foo"), Identity]`) und List-Literal als Konstruktor-Argument (`FunctionCall("__reduce__", [...])`).

**Regressionstest:** `tests/e2e/bug10_mixed_enum_list_literal.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
let args: List<Filter> = [
    Filter::Identity,
    Filter::LiteralStr("x"),
    Filter::FieldAccess("foo"),
    Filter::Identity,
];
// args[0], args[1] etc. enthalten falsche/korrupte Werte
```

**Ursache:**
List-Literal `[a, b, c, d]` für `List<EnumType>` speichert Elemente korrupt,
wenn die Enum-Varianten unterschiedliche Payload-Typen haben (gemischte Typen).

**Workaround in jgrep:**
```tinox
var args: List<Filter> = [];
args.push(Filter::Identity);
args.push(Filter::LiteralStr("x"));
// usw.
```

**Erwartetes Verhalten:**
List-Literal `[a, b, c, d]` soll korrekt funktionieren, auch bei gemischten Enum-Varianten.

---

## Bug 11 — `parseComma()` für reduce/foreach-Generator konsumiert `as $x` gierig

**Status: KEIN TINOX-BUG (2026-07-05)** — `reduce ... as $x (...)` ist jq-Syntax und wird von jgreps eigenem Filter-Parser (filter.tnx) geparst, nicht von `crates/tinox-parser/` (dort existiert kein reduce/as). Der unten beschriebene Workaround ist der eigentliche Fix und ist in jgrep bereits umgesetzt. Im Tinox-Repo nichts zu tun.

**Datei:** ~~`crates/tinox-parser/`~~ → jgreps Filter-Parser (`/home/tg7c49/git/jgrep-tinox`)

**Problem:**
```
reduce .[] as $x (0; . + $x)
```
Wird falsch geparst: `parseComma()` → `parsePipe()` → `parseAs()` konsumiert das
`as $x`-Muster als Teil des Generator-Ausdrucks `.[]`, statt es dem reduce-Syntax zu überlassen.

**Workaround in jgrep:**
In `parseReduce()` und `parseForeach()` `parseAlternative()` statt `parseComma()` für den
Generator-Ausdruck verwenden.

**Erwartetes Verhalten:**
`reduce EXPR as $x (INIT; BODY)` soll korrekt geparst werden.

---

## Bug 12 — Compound-Assignment-Operatoren werden nicht erkannt

**Status: GEFIXT (2026-07-05)** — Lexer (`PlusEquals` etc.) und Parser konnten es bereits; die eigentlichen Fehler lagen woanders: (a) Codegen `gen_compound_assign` (Ident-Zweig) verwendete den rohen Variablennamen `%x` statt des versionierten Slots aus `ctx.local_slots` (→ "use of undefined value"), (b) der Statement-Parser kannte Compound-Assign nicht für Index-Targets (`lst[i] += v` war Parse-Fehler). Zusätzlich: Float-Compound-Assign emittiert jetzt fadd/fsub/… statt ungültigem `add double`, und `s += t` auf Strings konkateniert via `@tinox_string_concat`. Verifiziert mit Int/Float/String/List-Index. Hinweis: `//=` gibt es in Tinox nicht (`//` ist Kommentar) — das betrifft nur jgreps jq-Filter-Lexer.

**Regressionstest:** `tests/e2e/bug12_compound_assign.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs` + `crates/tinox-parser/src/parser.rs` (~~`crates/tinox-lexer/`~~ war korrekt)

**Problem:**
```tinox
var x = 0;
x += 1;   // Wird als Add(x, null) = 1 geparst → Fehler
x -= 1;   // gleicher Fehler
x *= 2;
x /= 2;
x %= 3;
x //= default;
```

**Ursache:**
Der Lexer erzeugt für `+` nur den `Plus`-Token und prüft nicht, ob ein `=` folgt.
Damit fehlen `PlusAssign`, `MinusAssign`, `MulAssign`, `DivAssign`, `ModAssign`, `AltAssign`.

**Workaround in jgrep:**
Lexer in der jgrep-eigenen filter.tnx (Filter-Parser) um diese Tokens erweitert.
Der Tinox-Sprach-Lexer selbst wurde nicht geändert, da jgrep seinen eigenen Lexer hat.

**Erwartetes Verhalten:**
`+=`, `-=`, `*=`, `/=`, `%=`, `//=` sollen als eigene Tokens erkannt und als
Compound-Assignment-Statements geparst werden.

---

## Bug 13 — `sv + bs` auf match-gebundenen Strings macht Integer-Addition

**Status: GEFIXT (2026-07-05)** — Durch denselben Fix wie Bug 1: beide Operanden sind `i8*`, `+` emittiert String-Konkatenation.

**Regressionstest:** `tests/e2e/bug13_match_string_concat.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match a {
    Str(sv) =>
        match b {
            Str(bs) => return JsonValue::Str(sv + bs);
        }
}
```
Gibt einen korrupten Wert zurück (Pointer-Addition statt String-Konkatenation).

**Ursache:**
Ähnlich wie Bug 1/2: `sv` und `bs` sind `i64` (match-bound). Der `+`-Operator prüft
`lt == "i8*" || rt == "i8*"` für String-Concat — bei zwei `i64`-Operanden ist diese
Bedingung `false`, daher wird `add i64` emittiert (Pointer-Addition).

**Workaround in jgrep:**
```tinox
fn strConcat(a: String, b: String) -> String { return a + b; }
// Verwendung: strConcat(sv, bs) statt sv + bs
```

**Erwartetes Verhalten:**
`sv + bs` für zwei Strings soll `@tinox_string_concat` aufrufen.

---

## Bug 14 — `s.contains(needle)` auf match-gebundenen Strings geht durch Map-Dispatch

**Status: GEFIXT (2026-07-05)** — Durch denselben Fix wie Bug 1: `obj_ty` ist jetzt `i8*`, der Aufruf geht durch String-Dispatch statt Map-Dispatch.

**Regressionstest:** `tests/e2e/bug14_match_string_contains.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs`

**Problem:**
```tinox
match v {
    Str(s) => return s.contains("hello");
}
// Ruft @tinox_map_contains auf statt @tinox_string_contains
// Gibt falsches Ergebnis zurück (behandelt den String-Pointer als Map)
```

**Ursache:**
`is_map_dispatch` (Zeile ~4974) prüft `obj_ty == "i64" && method in {..., "contains", ...}`.
Match-gebundene Strings sind `i64` → der `contains`-Aufruf geht fälschlicherweise durch
Map-Dispatch, weil die Prüfung nicht zwischen match-gebundenen Maps und Strings unterscheidet.

**Workaround in jgrep:**
```tinox
fn strContains(haystack: String, needle: String) -> Bool { return haystack.contains(needle); }
```

**Erwartetes Verhalten:**
`s.contains(needle)` auf einem match-gebundenen String soll `@tinox_string_contains` aufrufen.

---

## Bug 15 — `.len()` auf List<String>-Elementen aus Funktionsergebnissen geht durch Map-Dispatch

**Status: GEFIXT (2026-07-06)** — Entdeckt beim ygrep-Port (YAML-Parser in jgrep-tinox).

**Regressionstest:** `tests/e2e/bug15_string_list_elem_typing.tnx`

**Problem:**
```tinox
fn splitLines(s: String) -> List<String> { ... }

let lines = splitLines(content);
lines[0].len();          // ruft tinox_map_len auf dem String-Pointer auf → Müll
let raw = this.rawLines[i];
raw.len();               // dito für List<String>-Felder
```
Die Let/Var-Inferenz kannte `Array:String` nur für `split()`/`keys()`-Aufrufe (Bug 8).
Funktions-/Methodenaufrufe mit deklariertem Rückgabetyp `List<String>` sowie
`List<String>`-Felder blieben untypisiert; Elemente wurden als `i64` behandelt, und
`.len()` auf `i64` landet im Map-Dispatch (`tinox_map_len` liest Bytes 8–16 des Strings).
Je nach Heap-Layout „funktionierte" das zufällig (substring clampt), oder Zeilen
verschwanden (bei 15/16-Zeichen-Strings war das gelesene Längenfeld 0).

**Fix (mehrteilig, `codegen.rs`):**
1. Pre-Pass registriert Methoden und Top-Level-Funktionen mit Rückgabetyp
   `List<String>`/`Array<String>` in `method_ret_class` als `"Array:String"`
   (Helper `is_string_list_type`).
2. `inferred_struct`-Arme (let + var) schlagen jetzt auch für `ExprKind::Call`
   in `method_ret_class` nach.
3. `extract_class_type_name` liefert für String-Listen `"Array:String"`, und der
   Index-Codegen fällt für Nicht-Ident-Objekte (z. B. `this.feld[i]`) auf
   `infer_struct_type` zurück → Elemente von `List<String>`-Feldern sind i8*.
4. Folgefix: Der `"Array:String:elem"`-Marker von for-Schleifen wird nach
   Schleifenende entfernt, und Let/Var-Neubindungen ohne Typ-Info löschen
   veraltete `local_types`-Einträge (`local_types` ist funktionsflach; vorher
   erbte eine spätere Variable gleichen Namens den Marker und bekam ein
   ungültiges `inttoptr i64` auf einen i8*-Load → llc-Fehler).

---

## Bug 16 — String-Literal mit führendem `#` wird als Raw-String gelext

**Status: GEFIXT (2026-07-06)** — Entdeckt beim ygrep-Port (YAML-Kommentar-Tests).

**Regressionstest:** `tests/e2e/bug16_hash_string_literal.tnx`

**Problem:**
```tinox
let s = "# top\na: 1";   // \n bleibt als Backslash+n im String stehen
```
Der Lexer behandelte `"` gefolgt von `#` als Raw-String-Beginn
(`read_raw_string`, gedacht für `r#"…"#`). Ein normaler String, dessen erstes
Zeichen `#` ist, verlor dadurch die komplette Escape-Verarbeitung.

**Fix:** Sonderfall im `'"'`-Arm des Lexers entfernt — Raw-Strings sind immer
`r`-präfigiert (`r"…"`, `r#"…"#`); `"#…"` ist ein normales String-Literal.

---

## Bug 17 — Test-Harness liest Bool-Rückgabe als i64: fehlschlagende Tests werden PASS

**Status: GEFIXT (2026-07-07)** — Entdeckt bei der jgrep-Performance-Arbeit; hatte
in der jgrep-Suite zwei echte Fehlschläge verdeckt (`flatten(1)`, NDJSON-Recovery).

**Regressionstest:** `tests/e2e/bug17_test_harness_bool.tnx`

**Problem:**
`emit_test_code()` rief die `@Test`-Methode als `call i64 @Class_method(...)` auf
und prüfte `icmp ne i64 %result, 0`. Die Methode ist aber als `i1` definiert
(Bool-Rückgabe). Der ABI-Mismatch liest die oberen 63 Bits von `%rax` als
undefinierten Müll: nach `return xs.len() == 999;` steht dort z. B. noch der
Wert von `len()` — Ergebnis ≠ 0 → Test „bestanden", obwohl er `false` liefert.
`return false;` als einziger Ausdruck fiel dagegen korrekt durch (xor eax,eax).
Effekt: Tests, deren letzter Ausdruck ein Vergleich nach vorheriger Berechnung
ist, können praktisch nie fehlschlagen.

**Fix:** Aufruf als `call i1` + direktes `select i1` (codegen.rs, `emit_test_code`).

**Merkposten:** Nach dem Fix jede bestehende Suite einmal neu laufen lassen —
vorher „grüne" Tests können echte Fehlschläge gewesen sein.

---

## Notiz — `tinox_array_push` ist O(n) pro Push (kein Kapazitäts-Überhang)

**Status: GEFIXT (2026-07-09)** — Array-ABI auf stabile Handles umgestellt.
Ein Array-Wert ist jetzt ein `i64*` auf einen 3-Slot-Header `{len, cap, data}`;
der Element-Buffer hängt an Slot 2 und wächst geometrisch (Verdopplung,
min. 4). `push`/`pop`/`removeAt` mutieren das Handle in place → Push ist
amortisiert O(1), Pop O(1). `slice`/`sort`/`reverse` liefern weiterhin frische
Arrays. Benchmark: 100k Pushes 2,53 s → ~1 ms; 1 Mio. Pushes in 5 ms
(Regressionstest `array_push_1m_o1` in tests/runtime_tests.sh).

**Semantik-Änderung:** Listen haben jetzt durchgängige **Referenz-Semantik** —
Aliase (`let b = a`, Funktionsparameter, Struct-Felder) teilen das Handle und
sehen Pushes/Pops. Vorher war die Semantik inkonsistent (Element-Zuweisungen
und `removeAt` in place sichtbar, Pushes nicht) — genau diese Inkonsistenz
hatte den GC_size-Versuch nichtdeterministisch korrumpiert. Die jgrep-Suite
(170 Tests) läuft unverändert grün; Semantik-Tests:
`array_alias_reference_semantics`, `array_copy_methods_fresh`.

**Dabei mitgefixt (latenter Bug derselben Klasse):** `xs.push(v)` auf einem
*Funktionsparameter* emittierte einen Pointer-Write-back durch den rohen
SSA-Wert (`store i64* %new, i64** %xs` — %xs ist aber kein Slot) und
überschrieb damit Element 0 des Caller-Arrays. Mit stabilen Handles ist der
Write-back ersatzlos entfernt (push/removeAt geben dasselbe Handle zurück).
Außerdem: `sort()` gab einen thread-lokalen Shared-Buffer zurück (zweiter
sort()-Aufruf korrumpierte das erste Ergebnis) — jetzt frische Allokation.
Ebenso `processArgs()`-Elemente als String typisiert (gleiche Lücke wie
Bug 7/dirList).

Weiterhin offen (unverändert): `tinox_string_substring` und
`tinox_string_length` machen `strlen` über den ganzen String — Lexer, die
zeichenweise über einen großen Quellstring laufen, müssen die Länge cachen und
`charAt` (O(1), ohne strlen) statt `substring(i, i+1)` benutzen.

## Bug 18 — Klassen-Payloads aus match-Bindungen: Feldzugriff las Offset 0

**Status: GEFIXT (2026-07-11)** — Gefunden beim Testen der typisierten
Match-Payload-Bindungen (TESTPLAN Phase 4); die Kontext-Matrix hatte keine
Klasseninstanzen als Subjekt-Typ (inzwischen ergänzt: TypeSpec `user`).

**Regressionstest:** `tests/e2e/bug18_class_payload_field.tnx`

**Problem:**
```tinox
enum Res { Ok(User), Err(String) }
match r {
    Ok(u) => println(u.name);  // druckte 7 (die id) statt "Alice"
    ...
}
```
`payload_kind()` klassifiziert Klassen-Payloads als "Other" —
`bind_match_payload` bindet `u` als nacktes i64 ohne `local_types`-Marker.
Der FieldAccess-Codegen fand keinen Klassennamen, fiel auf Offset 0 mit
i64-Typisierung zurück: `u.name` las das erste Feld (id) und druckte es
als Zahl. Methodenaufrufe (`u.greet()`) waren aus demselben Grund kaputt.

**Fix:** Der FieldAccess-Arm konsultiert als Fallback die
Typecheck-Tabelle (`expr_markers`), die seit den typisierten
Match-Payload-Bindungen den Klassennamen kennt (Ident-Fallback im
Methoden-Dispatch existierte schon). Kein neuer Codegen-Sonderfall.

---

## Bug 19 — `Class::method(obj, …)` auf Nothing-Methoden: benannter void-Call

**Status: GEFIXT (2026-07-17)** — Gefunden vom neuen Stdlib-Smoke-Gate
(`crates/tinox/tests/stdlib_smoke.rs`); blockierte allein ~8 Stdlib-Module
(Trie, Env, Bitmap, Debug, Logger, …).

**Regressionstest:** `tests/e2e/bug19_static_void_call.tnx`

**Datei:** `crates/tinox-codegen/src/codegen.rs` (EnumValue-Static-Call-Pfad)

**Problem:**
```tinox
class C {
    fn voidy(c: C, x: Int64) -> Nothing { println(x); }
}
C::voidy(c, 5);   // ICE: "instructions returning void cannot have a name"
```
Der Static-Dispatch-Pfad (`ExprKind::EnumValue` mit `method_ret_types`)
emittierte immer `%tmp.N = call <ret> @Class_method(...)` — bei `void`
ist das ungültiges IR. Der generische Pfad (`gen_generic_method_call`)
hatte den void-Sonderfall bereits.

**Fix:** `ret_ty == "void"` → unbenannter Call, Rückgabe `("0", "void")`.

---

## Bug 20 — Stdlib-Großbefund: 42 von 61 Modulen kompilieren nicht oder rechnen falsch

**Status: GEFIXT (2026-07-18)** — alle 61/61 Module grün, KNOWN_BROKEN leer
(Bugs 21-32). Befund des neuen Stdlib-Smoke-Gates
(`crates/tinox/tests/stdlib_smoke.rs`, Teil von `make e2e`). Kein Test hatte
diese Module je importiert; da ein Import das ganze Modul codegen't, reichte
ein minimaler Aufruf pro Modul, um alles Folgende aufzudecken. Die exakte
Liste steht in `KNOWN_BROKEN` im Test — jedes gefixte Modul MUSS dort
ausgetragen werden (sonst „stale entry"), jedes neue Modul braucht einen
Smoke-Fall (Vollständigkeits-Test).

Klasse 2 (Generics) ist seit 2026-07-17 gefixt — siehe Bug 21. Dabei ist
`pool` neu in KNOWN_BROKEN gelandet: es war vorher nur „grün", weil `Pool<T>`
mangels Generics-Infrastruktur nie erfolgreich spezialisiert wurde — der
eigentliche Modul-Bug (`pool.factory()` ohne deklariertes Feld) war
unerreichbar. Siehe Bug 22.

Klasse 1 (Ghost-Builtins) ist **vollständig** gefixt — siehe Bug 23 (13 Module:
mathf, debug, process, fs, time, string, io, metrics, random, regex,
base64, uri, uuid), Bug 24 (crypto, jwt) und Bug 25 (socket, http, rest,
xml, zip). `hex` (Klasse 6) fiel bei Bug 23 mit, gleiches Bug-Muster.
Klasse 3 (Casts) ist seit Bug 26 gefixt (complex/cron/decimal/fmt/toml).
Die Laufzeit-Fehlverhalten-Gruppe (asm/graph/heap/iter/queue/ratelimit/set)
und pool sind seit Bug 27 gefixt, `ini` seit Bug 28, `logger` seit Bug 29,
`events` seit Bug 30, `rest_framework` seit Bug 31, `http2_server` seit
Bug 32. **Stand: 61/61 grün, KNOWN_BROKEN leer — Bug 20 vollständig
abgearbeitet.**

**Grün verifiziert (37):** array, base64, bitmap, cache, collections,
crypto, csv, debug, encoding, env, format, fs, hash, hex, hpack,
http_server, io, json, jwt, math, mathf, mathx, metrics, option, process,
random, regex, result, semaphore, sort, string, time, tpl, trie, uri,
uuid, validation, yaml.

**Fehlerklassen (24 kaputt):**

1. **Ghost-Builtins** — Modul ruft Funktionen, die weder in `runtime/runtime.c`
   existieren noch im Codegen deklariert sind → ICE „use of undefined value".
   Größtenteils gefixt, siehe Bug 23/24. Verbleibend:
   - http: `httpGet/httpPost/httpPut/httpDelete/httpPatch/httpSetHeader/httpClearHeaders/httpStatusCode/httpBody/httpHeader`
   - socket: `socketConnect/socketBind/socketListen/socketSend/socketReceive/socketClose`
   - xml: `xmlTagName/xmlAttr/xmlChildren/xmlTextContent` · zip: `zipListEntries/zipExtractFile/zipAddFile/zipRemoveFile`
   - rest: transitiv kaputt (http-Ghosts)
2. ~~**Generics**~~ — **gefixt, siehe Bug 21.**
3. **Ungültige Casts** — `ptr → i64/double` (z. B. `sitofp i8* %value to double`)
   in Modul-Klassen mit String/Float-Feldern: complex, cron, decimal, fmt, toml.
4. **Lambda-/Handler-Codegen** — `%handler` i64 vs. ptr (events),
   Typ-Mismatch (logger), „unable to create block named 'entry'" (rest_framework).
5. **Frontend** — http2_server.tnx parst nicht (Zeile 770, „expected
   Semicolon, found Equals"); ini.tnx Typecheck-Fehler (`Map == null`).
6. **Laufzeit-Fehlverhalten** — kompiliert, rechnet falsch: heap/set
   (Pointer statt Wert), iter (`repeat(7,3).len()` → 7), graph (961 statt 1),
   queue (leere Ausgabe), asm/ratelimit (Crash, Exit -1). (hex gefixt, s. Bug 23.)
7. **Modul-Bug, durch Bug 21 aufgedeckt** — pool (siehe Bug 22).

**Empfohlene Reihenfolge:** Klasse 3–4 sind Compiler-Bugs (fixen), Klasse 1
ist eine Produktentscheidung pro Modul (Runtime-Funktion implementieren oder
Modul streichen), Klasse 5–7 sind Modul-Bugs im .tnx-Code.

---

## Bug 21 — Generische Klassen: Instanzmethoden nie emittiert/gebunden

**Status: GEFIXT (2026-07-17)** — Teilfund von Bug 20 (Klasse 2), eigener
Eintrag wegen Umfang. Betraf jede generische Klasse (`class Foo<T>`) mit
mehr als trivialen Fabrikmethoden — vier Fehlerquellen, alle im selben
Mechanismus (`ensure_generic_class_specialization*` in codegen.rs) bzw. in
der typecheck-Signaturregistrierung:

1. **Codegen: generische Klassen wurden komplett aus der normalen
   Methoden-Vorabregistrierung ausgeklammert** (`if !c.type_params.is_empty()
   { continue; }`) und nur über `New`-Ausdrücke (`new Foo<Int64>(...)`)
   monomorphisiert — nie über den in der Stdlib tatsächlich verwendeten
   Stil `Foo::method(args)` (`Option::some(5)`, `Cache::set(cache, k, v)`).
   Fix: `ExprKind::EnumValue`-Codegen leitet Typbindungen jetzt aus (a) der
   `let`-Annotation des Empfängers, (b) dem tatsächlichen Argumenttyp für
   `T`-typisierte Parameter, (c) dem Marker eines bereits spezialisierten
   Empfänger-Arguments (`cache: Cache<K,V>`) ab und monomorphisiert bei
   Bedarf on demand (`ensure_generic_class_specialization_with_bindings`,
   `emit_static_dispatch_call`).
2. **Codegen: Methoden-BODIES wurden nie typsubstituiert**, nur Feld-/
   Param-/Rückgabetypen (`substitute_class`). Ein `let value: V = ...;` im
   Body (z. B. `Cache::get`) behielt den nackten Typparameter — `V` fiel im
   Codegen auf `i64*` zurück, unabhängig vom tatsächlichen Bindungstyp.
   Fix: `substitute_stmt`/`substitute_expr` (neu) laufen einmal über den
   ganzen Methoden-Body jeder Spezialisierung.
3. **Codegen: Selbstreferenzen blieben unmangled.** `ClassName<T> { … }`
   (StructLiteral — hat kein `type_args`-Feld) und
   `cache: Cache<K,V>`-Parameter referenzierten weiter den unspezialisierten
   Klassennamen. `Result::ok(5)` allozierte dadurch eine 0-Byte-Struktur
   (`struct_layouts.get("Result")` war leer, da generisch → nie registriert)
   und lieferte Datenmüll (`0` statt `5`); `cache.accessOrder.removeAt(0)`
   auf einem `Cache<K,V>`-Parameter fand keinen Klassennamen und rief eine
   nicht existierende globale Funktion `@removeAt` auf. Fix: `rename_self_type`
   kollabiert jede Erwähnung des generischen Klassennamens (in Feld-/Param-/
   Rückgabetypen UND im Body — StructLiteral, New, EnumValue) auf den
   konkreten mangled Namen.
4. **Typecheck: nur methoden-eigene Typparameter wurden zu `Any` erased**,
   nie die der umschließenden Klasse. `push(value: T)` in `class Stack<T>`
   registrierte ein Signatur-Param `Named("T")` statt `Any` — jeder reale
   Aufruf `s.push(7)` schlug mit „expected T, found Int64" fehl. Dieselbe
   Lücke existierte doppelt (Top-Level- und Namespace-Klassenregistrierung)
   und ein drittes Mal in `check_class` (Body-Check: `cache.data[key]` mit
   `key: K` scheiterte an der Map-Index-Gültigkeitsprüfung, die `String`
   oder `Any` verlangt). Fix: alle drei Stellen erasen jetzt
   `c.type_params.chain(method.type_params)`.

**Regressionstest:** `crates/tinox/tests/stdlib_smoke.rs` (option, result,
cache, collections aus `KNOWN_BROKEN` entfernt — jeder erneute Fehlschlag
ist eine echte Regression). Kein dediziertes `tests/e2e/bugNN_*.tnx`, da der
Smoke-Test bereits die Minimalfälle abdeckt.

**Verbleibende Lücken (bewusst nicht mitgefixt, kein Regressionsrisiko für
die 4 Zielmodule):** Methoden mit `fnc(T) -> U`-Lambda-Parametern
(`Option::orElse`) und Methoden mit eigenen Typparametern (`Option::map<U>`)
werden bei der Spezialisierung übersprungen statt eagerly emittiert — beide
brauchen mehr als reine Signatur-Übersetzung und sind in keinem der vier
Module am Smoke-Pfad. `gen_generic_method_call` (Methoden-eigene Generics,
z. B. `Json::serialize<T>`) hat dieselbe Body-Substitutions-Lücke wie
ursprünglich `substitute_class` — absichtlich nicht angefasst, um die
bestehenden, bereits grünen Tests dafür nicht zu risikieren.

---

## Bug 22 — `Pool<T>.factory()` ohne deklariertes Feld

**Status: OFFEN (2026-07-17)** — Modul-Bug in `crates/tinox-core/pool.tnx`,
aufgedeckt durch Bug 21 (`Pool<T>` wurde vorher nie erfolgreich
spezialisiert, der Fehler war unerreichbar).

**Problem:**
```tinox
class Pool<T> {
    fn new(maxSize: Int64) -> Pool<T> {
        return Pool<T> { available: [], inUse: [], maxSize: maxSize };
    }
    fn acquire(pool: Pool<T>) -> T {
        ...
        let obj: T = pool.factory();   // "factory" ist nirgends deklariert
        ...
    }
}
```
Weder als `var factory: fnc() -> T;`-Feld noch im StructLiteral von `new()`
gesetzt. `acquire()` kompiliert nur, wenn `Pool<T>` spezialisiert wird —
seit Bug 21 der Fall — und bricht dann mit „use of undefined value
@Pool__i64_factory" ab (der Method-Call-Codegen behandelt den unbekannten
Feldzugriff als regulären, nie registrierten Klassenmethodenaufruf).

**Workaround:** `pool` steht in `KNOWN_BROKEN`
(`crates/tinox/tests/stdlib_smoke.rs`).

**Erwartetes Verhalten:** `Pool<T>` braucht entweder ein
`newWithFactory(maxSize: Int64, factory: fnc() -> T) -> Pool<T>` mit
`var factory: fnc() -> T;`-Feld, oder `acquire()` muss ohne Factory
auskommen (z. B. `throw` statt Aufruf, wenn der Pool leer und voll ist).

---

## Bug 23 — Ghost-Builtins Klasse 1: erste Runde (12 Module gefixt)

**Status: TEILWEISE GEFIXT (2026-07-17)** — Bug 20 Klasse 1 (Ghost-Builtins)
abgearbeitet für: mathf, debug, process, fs, time, string, io, metrics,
random, regex, base64, uri, uuid. Zwei Muster, je nach Modul:

1. **Reine Namens-/Signatur-Mismatches** — die Runtime-Funktion existierte
   bereits, das `.tnx`-Modul rief nur den falschen Namen oder die falsche
   Arity: `fileDelete` → `deleteFile`, `__tinox_counter_inc` →
   `tinox_counter_inc` (metrics.tnx, vier Fälle), `sleep(ms)` → `sleep_ms(ms)`
   (`sleep` kollidiert mit libc), `randomInt(max)` → `randomInt(0, max)`
   (Runtime erwartet `[min, max)`).
2. **Echte Lücke, C-Runtime ergänzt** (`runtime/runtime.c` +
   `crates/tinox-codegen/src/codegen.rs`-Declares): `processId`, `gcCollect`,
   `memoryUsage`, `printStackTrace` (Boehm-GC/glibc-Backtrace), `now`
   (epoch-Millisekunden, `currentTimeSecs` gab nur Sekunden — jwt.tnx
   dividiert `Time::now() / 1000` und hätte sonst falsch gerechnet),
   `sleep_ms`, `randomInt`/`randomFloat` (POSIX `random()`, einmalig
   geseedet), `tinox_string_reverse`, `tinox_string_last_index_of`,
   `regexFindFirst`, `regexReplaceAll` (Replace-Loop mit Wachstum,
   `regexReplace` konnte nur den ersten Treffer).
3. **Pure Tinox statt Runtime-Funktion** (Vorbild `hex.tnx`, das den ganzen
   Algorithmus schon in `.tnx` hatte): `base64.tnx` (Encode/Decode komplett
   neu geschrieben, 3-Byte-Gruppen), `uri.tnx` (`encode`/`decode`/
   `*Component` — RFC-3986-Prozent-Encoding, `parse()` war schon pure Tinox),
   `uuid.tnx` (v4 via `Random::nextInt`, Version-/Variant-Nibbles gesetzt).
   Kein Runtime-Risiko, sofort end-to-end testbar.

**Nebenfunde, mitgefixt:**
- `hex.tnx` (Klasse 6, „Laufzeit-Fehlverhalten"): `nibbleToChar` benutzte
  `(n + 48).toString()` statt `fromCharCode(n + 48)` — stringifiziert die
  ASCII-Codezahl selbst statt das Zeichen zu erzeugen (`encode("A")` gab
  Müll statt `"41"`). Genau dasselbe Bug-Muster tauchte beim ERSTEN
  Schreiben von `uri.tnx` auf und wurde dort vor dem Commit gefangen.
- `cache.tnx`-Klasse „fehlende Felder" wiederholt sich: `Stopwatch` in
  metrics.tnx hatte `startNs` nie deklariert; `Uuid` hatte `value` nie
  deklariert. Beide nachgetragen.
- **Typecheck-Bug, unabhängig von Ghost-Builtins:** `ClassName::method()`
  für eine NICHT-statische (`fn`) Methode ohne explizite Argumente wurde in
  `infer_type`s `ExprKind::EnumValue`-Arm als Enum-Variantenkonstruktion
  fehlgedeutet (`is_static`-Heuristik prüfte nur `sig.params.first() !=
  Some("self")` und hatte für den `!is_static`-Fall keinen Rückgabepfad —
  fiel durch zur Enum-Fallback-Logik). `Debug::memoryUsage()` lieferte so
  `Named("Debug")` statt `Int64`: „expected Int64, found Debug". Betraf
  jeden zukünftigen Aufruf eines zero-arg-Instanzmethoden-Aufrufs im
  `Klasse::methode()`-Stil — nicht nur Debug/Process. Fix: die Signatur wird
  jetzt immer verwendet, sobald sie gefunden ist (kein `is_static`-Gate mehr).

**Noch offen (Bug 20 Klasse 1):** — nichts mehr. http/socket/rest/xml/zip
sind in Bug 25 (dritte Runde) erledigt.

---

## Bug 24 — Ghost-Builtins Klasse 1: crypto + jwt (2 weitere Module)

**Status: GEFIXT (2026-07-17)** — `crypto` brauchte echte Hash-Algorithmen
(keine Namens-Mismatches wie bei Bug 23), `jwt` brauchte zusätzlich zwei
JSON-API-Lücken.

**crypto.tnx:** `md5Hash`/`sha256Hash`/`hmacSha256Hash` in `runtime.c` neu
implementiert — MD5 (RFC 1321), SHA-256 (FIPS 180-4), HMAC-SHA256 (RFC 2104),
komplett selbstständig ohne OpenSSL-Abhängigkeit (passend zum Rest der
Runtime, die externe Libs nur opt-in über `tinox.toml` einbindet). Gegen
Standard-Testvektoren verifiziert: `md5("") = d41d8cd9…`, `md5("abc") =
900150983c…`, `sha256("") = e3b0c442…`, `sha256("abc") = ba7816bf…`,
HMAC-SHA256("The quick brown fox…", key="key") `= f7bc83f4…` — alle exakt
match.

`aesEncrypt`/`aesDecrypt` bewusst NICHT implementiert: ein XOR- oder
sonstiger Platzhalter unter dem Namen „AES" wäre eine stille
Sicherheitslücke (der Methodenname verspricht echte Verschlüsselung, ein
schwacher Ersatz würde das verdecken). Beide werfen jetzt klar
`"Crypto::aesEncrypt ist nicht implementiert"` statt vorzutäuschen.
`Crypto::pbkdf2` bleibt unangetastet — iteriertes `sha256(sha256(...))`
ist kein echtes PBKDF2 (kein HMAC pro Runde), aber das ist ein
Qualitäts-, kein Ghost-Builtin-Problem, außerhalb dieses Scopes.

**jwt.tnx:** fehlte komplett ohne `import`-Zeilen (gleiches Muster wie
Bug 21s Cache-Fund) — `base64`/`crypto`/`json`/`time` ergänzt. Rief
außerdem zwei nie existierende `JsonValue`-Methoden auf:
- `JsonValue::asTable() -> Map<String, JsonValue>` und
  `JsonValue::asInt() -> Int64` (Alias zu `getInt()`) in `json.tnx` ergänzt,
  über den neuen `extern fn jsonGetObject(...)`-Ghost-Fund (existierte
  schon in `runtime.c` als `jsonGetObject`, war nur nie deklariert).
- `Jwt::encode` reichte `Map<String, JsonValue>` direkt an
  `Json::stringify(value: JsonValue)` durch — beide haben zur Laufzeit
  unterschiedliche Speicherlayouts (`TinoxMap*` vs. `TinoxJsonValue*`),
  hätte Datenmüll gelesen. Neue Gegenstück-Funktion `Json::fromMap(map)`
  (→ `jsonFromMap` in `runtime.c`) wrappt eine bestehende Map als
  JSON_OBJECT-JsonValue — beide Maptypen (`tinox_map_create`,
  `json_obj_map_create`) teilen sich dieselbe `TinoxMap`-Struct, nur der
  Allokator unterscheidet sich, daher direkt wiederverwendbar.

**Wichtiger Fund dabei, NICHT gefixt (Scope-Grenze):** `Json::parse()`
liefert `JsonValue`-Zeiger in einen **`__thread`-lokalen Arena-Puffer, der
bei jedem `jsonParse()`-Aufruf zurückgesetzt wird** (Kommentar im Code:
„valid until the next call"). Ein Payload, der aus mehreren einzelnen
`Json::parse(...)`-Aufrufen zusammengebaut wird (z. B. pro Feld je ein
Parse), korrumpiert bereits gespeicherte `JsonValue`s beim nächsten Parse —
beobachtet als leerer String bei `decoded["sub"].getString()`, sobald
danach `Json::parse("9999999999")` für ein zweites Feld lief. **Kein**
Problem für den Hauptpfad (`Jwt::decode`/`extractPayload` parsen den
kompletten Payload in einem einzigen Aufruf — verifiziert, funktioniert
korrekt) oder für `Json::parse(vollständigerJsonString)` generell. Ist ein
generisches Lebensdauer-Risiko der gesamten `tinox.core.json`-API, nicht
JWT-spezifisch — verdient einen eigenen, gründlichen Fix (Arena
abschaffen oder GC-Heap-Fallback), nicht im Rahmen von „Ghost-Builtins"
nebenbei erledigt.

**Entdeckungsweg (Werkzeugkoffer für zukünftige `extern fn`-Fälle):** Tinox
unterstützt getypte Extern-Deklarationen (`extern fn name(args) -> Ret;`,
körperlose `fn`), die `gen_fn` automatisch in ein korrekt typisiertes
`declare` übersetzen — die von Bug 23 benutzten manuellen
`writeln!(...,"declare ...")`-Zeilen in `codegen.rs` sind ein Workaround
für Fälle ohne diesen Mechanismus. Für Klasse-1-Fixes ist `extern fn` im
`.tnx`-Modul selbst der sauberere Weg (typsicher, kein Rückgabetyp-Rateraten
über `arg_types.first()`) — bei `json.tnx`s bereits bestehenden
`extern fn jsonGetInt(...)` etc. entdeckt, für `jsonGetObject`/`jsonFromMap`
so verwendet.

---

## Bug 25 — Ghost-Builtins Klasse 1: dritte Runde (socket, http, rest, xml, zip)

**Status: GEFIXT (2026-07-18)** — die letzten fünf Ghost-Builtin-Module.
Alle über `extern fn`-Deklarationen im `.tnx` (der saubere Weg aus Bug 24),
neue Runtime-Funktionen in `runtime.c`, jeweils gegen echte Gegenstellen
verifiziert. Danach 38/61 → 43/61 grün (KNOWN_BROKEN 23 → 18).

**socket.tnx:** rohe BSD-Socket-Primitiven (`socketCreateTcp/Udp`,
`socketConnect/Bind/Listen/Accept/Send/Receive/Close`) über `<netdb.h>`
neu in `runtime.c`. `Socket` bekam `handle: Int64` + `mode: String`.
Beide Richtungen gegen einen Python-Peer getestet (Client: connect/send/recv;
Server: bind/listen/accept/recv/send).

**http.tnx:** blockierender HTTP/1.1-Client (nur `http://`, kein TLS) in
`runtime.c`. `TinoxHttpResponse{status, body, headers}` als opakes C-Struct,
das `HttpResponse` durchgereicht wird; Request-Header thread-lokal über
`httpSetHeader`. `httpGet/Post/Put/Delete/Patch`, plus
`httpStatusCode/httpBody/httpHeader` (Header-Lookup case-insensitive).
Gegen einen Python-`http.server` getestet: GET (Status 200, Body, Custom-
Header) und POST-Echo (Status 201) korrekt.

**rest.tnx:** benutzte `Http::`/`HttpResponse` ohne `import tinox.core.http`
— gleiches Import-Loch wie jwt in Bug 24. Import ergänzt, transitiv grün.

**xml.tnx:** zwei Probleme. (1) Die „Ghost-Builtins" `xmlTagName/
xmlTextContent/xmlAttr/xmlChildren` waren in Wahrheit reine Feldzugriffe —
`XmlNode` bekam die fehlenden Felddeklarationen (`tagName/text/attrs/
children`), die Methoden lesen jetzt `this.<feld>` statt der Geister.
(2) **Compiler-Fund:** `Strings::trim(x)` (statischer Aufruf auf einer
**nicht importierten** Klasse) erzeugte stillschweigend Datenmüll statt
eines Typfehlers — `import tinox.core.string` ergänzt. Der Compiler sollte
statische Aufrufe auf unbekannte/nicht-importierte Klassen ablehnen; tut er
nicht (eigenes, größeres Frontend-Thema, hier nur umschifft). Betrifft
potentiell auch `toml`/`ini`/`decimal` (nutzen `Strings::` ebenfalls ohne
Import — stehen aus anderen Gründen noch in KNOWN_BROKEN).

**zip.tnx:** echter, minimaler ZIP-Reader/-Writer in `runtime.c` — STORED
(Methode 0, keine Kompression), gültige Local/Central-Directory/EOCD-Records
mit korrekter CRC-32 (RFC 1952 / ISO-HDLC). Von System-`unzip -t`
verifiziert („No errors detected"), inklusive verschachtelter Pfade
(`dir/b.txt`). Bewusste Grenzen: nur Textinhalte (Tinox-Strings sind
nullterminiert, echte Binärdaten mit Nullbytes nicht darstellbar); beim
Lesen nur STORED (deflate-komprimierte Einträge werden übersprungen, da
kein zlib-Link). **ABI-Entkopplung:** `zipListEntries` konstruiert die
`List<ZipEntry>` **nicht** in C (das würde von der Klassen-Speicherlayout-
ABI abhängen — Vtable-Slot ja/nein, Feldreihenfolge), sondern C liefert nur
Skalare (`zipEntryCount/zipEntryName/zipEntrySize`) und die Tinox-Seite baut
die Liste selbst. Sauberer Trennschnitt für alle künftigen „C gibt
Objektlisten zurück"-Fälle.

---

## Bug 26 — Klasse 3 (Casts): complex, cron, decimal, fmt, toml + zwei Codegen-Grundfixes

**Status: GEFIXT (2026-07-18)** — die fünf „Cast"-Module. Diagnose ergab drei
verschiedene Ursachen; zwei davon waren echte, allgemeine Codegen-Bugs (nicht
modulspezifisch), die im gesamten Compiler wirkten.

**Codegen-Grundfix 1 — Ternär/`if`-Ausdruck verlor den Zweig-Typ.** Der
`If`-Ausdruck (auch `cond ? a : b`) allozierte immer `i64` und speicherte die
Zweigwerte **roh** ohne Konvertierung. Bei `i8*`/`double`/`i1`-Zweigen ergab
das ungültige IR (`store i64 <ptr>` bzw. Typ-Mismatch) oder — falls es doch
durchkam — falsche Werte (Integer-Bits statt Pointer/Float). Fix: Zweige über
`coerce_to_i64` in die Uniform-i64-Zelle schreiben, am Merge-Punkt zurück-
casten (`inttoptr`/`bitcast`/`trunc i1`), Ergebnistyp aus den Zweigen führen.
Betraf jeden String-/Float-/Bool-Ternär im ganzen Code, nicht nur `complex`
(dort `Complex::toString`s `(c.imag >= 0.0) ? "+" : "-"`).

**Codegen-Grundfix 2 — `(Int64)str` / `(Float64)str` erzeugte Unsinn.** Der
Cast-Pfad kannte keine String-Quelle: `(Int64)"5"` fiel in den Integer-Zweig
(`trunc i8* to i64` — ungültig), `(Float64)"1.5"` in `sitofp i8*`. Fix: für
`val_ty == "i8*"` jetzt **parsen** — `@tinox_string_to_int` bzw.
`@tinox_string_to_float` (mit `fptrunc`/`trunc` auf Zielbreite). Betraf
`cron` (`(Int64)field`), `toml` (`(Int64)/(Float64)value`), `decimal`,
`ini`. Numerische Casts (`(Int64)double`, `(Float64)int`) liefen schon.

**Modul-Einzelfixes:**
- `complex`: (a) `import tinox.core.mathf` fehlte (gleicher Ghost-durch-
  fehlenden-Import-Bug wie xml/Strings in Bug 25 — `Mathf::cos` wurde still
  als Struct-Literal fehlkompiliert). (b) `Complex` hatte **keine Feld-
  deklarationen** (`real`/`imag: Float64`) — ohne sie defaulten Feldtypen auf
  `i64`, `c.real * c.real` wurde Integer-Mathe auf den Double-Bits (kompilierte
  bei `magnitude`, rechnete Müll; `multiply` brach ganz). (c) `Mathf::atan2`
  und `Mathf::exp` existierten nicht → in `mathf.tnx` ergänzt (rufen die
  Builtins `atan2`/`exp`), plus `atan2`-Registrierung (Typecheck 2-arg,
  Codegen `declare double @atan2(double,double)` + Dispatch-Case).
- `decimal`: fehlende Felddeklarationen (`value: String`, `scale: Int64`) +
  `import tinox.core.string` (für `Strings::repeat`).
- `fmt`: **kein Modul-Bug** — der Smoke-Fall war falsch (`{}`-Platzhalter,
  aber `Fmt::sprintf` ist printf-artig mit `%s`). Testkörper auf `"a%sb"`
  korrigiert.
- `cron`/`toml`: allein durch Codegen-Grundfix 2 grün.

Verifiziert: `magnitude(3,4)=5`, `Decimal::toString(fromInt(3))="3"`,
`sprintf("a%sb",["X"])="aXb"`, cron/toml Smoke grün. Stand: 43/61 → 48/61
(KNOWN_BROKEN 18 → 13). **Die zwei Grundfixes sind allgemeine Verbesserungen**
— jeder String/Float/Bool-Ternär und jeder String→Zahl-Cast im ganzen
Projekt profitiert; `make check` voll grün (keine Regression).

**Offen (bewusst nicht hier):** `ini` nutzt `(Int64)strVal` (jetzt via
Grundfix 2 sauber) UND `Strings::` ohne Import — steht aus Klasse-5-Gründen
(Frontend-Parsefehler) weiter in KNOWN_BROKEN.

---

## Bug 27 — Laufzeit-Fehlverhalten-Gruppe + pool (asm/graph/heap/iter/queue/ratelimit/set)

**Status: GEFIXT (2026-07-18)** — die 7-Modul-Gruppe „falsche Werte/Crash"
plus `pool`. Diagnose: drei allgemeine Codegen-/Typecheck-Bugs + das schon
bekannte Feld-/Import-Muster. Stand: 48/61 → 56/61 (KNOWN_BROKEN 13 → 5).

**Codegen-Grundfix 1 — generische Klassen ohne Basis-Layout.** Die Klassen-
Vorabregistrierung überspringt generische Klassen komplett (`if
!c.type_params.is_empty() { continue; }`), Layouts entstehen nur pro
Spezialisierung unter gemangeltem Namen. Ein bare `Foo { … }`-Literal (Typ-
Argumente elidiert, z. B. `PriorityItem { … }` innerhalb `PriorityQueue<T>`,
wo T bereits erased ist) löst aber auf den Basis-Namen auf → `tinox_alloc(0)`,
alle Felder auf Offset 0 (überschreiben sich). Fix: für generische Klassen den
typ-erased Basis-Layout registrieren (T → i64*). Betraf queue (PriorityItem).

**Codegen-Grundfix 2 — Container-Marker für generische Element-Typen.**
`container_marker` kannte nur `Named`-Elemente (`List<Foo>` → „List:Foo"),
nicht generische (`List<PriorityItem<T>>`). Ohne Marker liefert `xs[0]` rohe
i64 statt Klassenzeiger → `.item` dereferenziert Müll (Crash). Fix: generische
Klassen-Elemente markern per Basis-Name (Container-Keywords List/Array/Map
fallen weiter in den rekursiven Zweig, damit verschachtelte Listen komponieren).

**Codegen-Grundfix 3 — self-Verschiebung bei generischen Methodenaufrufen.**
`Iter::repeat(7,3)` band `count=7, value=null`: der Call-Site stellte für
nicht-statische generische Methoden `i64* null` (self) voran, aber die
Definition wird über `gen_fn` als top-level-Funktion OHNE self emittiert →
alle Argumente um eins verschoben. Dieser Pfad ist ausschließlich der
statische `Class::method`-Aufruf (Instanzaufrufe laufen woanders), also self-
Push entfernt. Betraf iter.

**Typecheck-Fix — Typparameter-Erasure rekursiv.** `type_to_value_erasing`
löschte nur einen Typparameter als Ganzes (`T` → Any), nicht verschachtelt.
Eine generische Methode mit Rückgabe `List<T>` unifizierte daher nicht mit
konkretem `List<Int64>` (Fehler „expected List<Int64>, found List<T>"). Fix:
rekursiv in Array/List/Map absteigen → `List<T>` erased zu Array(Any). Betraf
iter (`repeat<T>`).

**Kleinerer Codegen-Fix — `removeAt` fehlte in `array_only_methods`.** Es
hatte einen Dispatch-Case, stand aber nicht in der Liste, die die Array-
Methoden-Dispatch für i64-geladene Felder auslöst → `set.items.removeAt(i)`
wurde als Ghost `@removeAt` emittiert. Betraf queue, set.

**Modul-Fixes (Feld-/Import-/pop-Muster):**
- Felddeklarationen ergänzt: `Assembler` (bytecode/labels), `Graph<V>`
  (nodes/edges), `RateLimiter`, `TokenBucket`, `Set<T>` (items),
  `PriorityQueue<T>` (items), `CircularBuffer<T>`, `Heap<T>`
  (items + comparator-Callback-Feld), `Pool<T>`.
- `ratelimit`: `import time`/`mathf` fehlten; `TokenBucket.tokens` mit
  `(Float64)capacity` initialisiert (Float-Feld, Int-Wert).
- `pop()`-Missbrauch: `pop()` gibt in Tinox das Array zurück, nicht das
  entfernte Element (bestätigt an der grünen collections.tnx: dort
  `data[len-1]` lesen, dann `pop()` als Statement). heap und pool lasen
  fälschlich `let x = items.pop()` → auf das collections-Idiom umgestellt.

**`pool` teilweise:** kompiliert + Smoke (new/release/clear) grün. Der
`acquire()`-über-`factory`-Pfad bleibt unbedienbar — ein als **Argument**
übergebenes Lambda wird als nackter fn-Zeiger gespeichert, während der
fn-Feld-Aufruf ein Closure-Struct `{fn_ptr, env}` erwartet (inline im
Struct-Literal übergebene Lambdas wie `Heap`s comparator werden dagegen als
Closure gewrappt). Diese Closure-Repräsentations-Inkonsistenz ist ein eigener,
tieferer Fix; `newWithFactory` wurde bewusst nicht eingebaut, um keine
segfaultende API zu versprechen.

**Verbleibende KNOWN_BROKEN (5):** events/logger/rest_framework (Klasse 4,
Lambda/Handler-Typen), http2_server/ini (Klasse 5, Frontend). `make check`
voll grün.

---

## Bug 28 — ini: Referenz `== null` im Typecheck erlaubt + fehlender Import

**Status: GEFIXT (2026-07-18)** — Stand: 56/61 → 57/61 (KNOWN_BROKEN 5 → 4).

**Typecheck-Fix (allgemein):** `check_binary_op` erlaubte `Eq`/`Ne` nur bei
`lhs == rhs`. `Named`-Typen (Klassen) fielen schon vorher durch den Wildcard-
Kurzschluss (`Any`/`Named` → skip), aber `Map`/`Array`/`String`/`Fn`/`Nullable`
`== null` wurde abgelehnt („binary op 'eq' cannot be applied to Map and null").
Da diese Typen als Zeiger gespeichert sind, ist der Null-Vergleich sinnvoll.
Neue Helper `is_nullable_ref` erlaubt `<ref> == null` / `null == <ref>` für
Referenztypen; Skalare (`Int`/`Float`/`Bool`/`Char`) werfen weiterhin (an
`5 == null` verifiziert). Codegen konnte den Zeiger-Null-Vergleich bereits.
Verifiziert: frische `Map`/`List`/`String` sind non-null; `ini` legt fehlende
Sektions-Maps korrekt an (`if sectionMap == null` feuert im richtigen Fall).

**Modul-Fix:** `ini` rief `Strings::trim` ohne `import tinox.core.string`
(dasselbe still-Datenmüll-Muster wie xml/decimal in Bug 25/26) — Import ergänzt.

**Verbleibende KNOWN_BROKEN (4):** events/logger/rest_framework (Lambda/
Handler-Typen), http2_server (Parser: verschachtelte Lvalue-Zuweisung
`map[key].field = val`).

---

## Bug 29 — logger: LogLevel-Wrapper-Objekte statt Int-Konstanten

**Status: GEFIXT (2026-07-18)** — reiner Modul-Fix. Stand: 57/61 → 58/61
(KNOWN_BROKEN 4 → 3).

`LogLevel::Debug()` etc. gaben Wrapper-Objekte (`LogLevel { value: 0 }`, also
`i64*`) zurück, wurden aber mit `<=` verglichen (`logger.level <=
LogLevel::Debug`) — `icmp sle i64 <feld>, <ptr>` = ungültiges IR. Außerdem
hatte `Logger` keine Felddeklarationen und rief `Time::now()` ohne Import.

Fix (analog zur grünen `asm`-`Ops`-Klasse mit Int-Konstanten):
- `LogLevel`-Methoden geben jetzt `Int64` (0..3) statt Wrapper zurück —
  Levels sind numerisch vergleichbar.
- `Logger` bekam `var name: String; var level: Int64;`, `setLevel`-Parameter
  `LogLevel` → `Int64`.
- `import tinox.core.time` ergänzt.

Verifiziert: Level-Filterung korrekt (debug unterdrückt bei level=Info,
`setLevel(Error)` unterdrückt danach info, error kommt durch), Zeitstempel
echt.

**Verbleibende KNOWN_BROKEN (3):** events/rest_framework (Lambda/Handler-
Typen), http2_server (Parser: verschachtelte Lvalue-Zuweisung).

---

## Bug 30 — events: Closure-Capture von Params + void-Lambda-Return

**Status: GEFIXT (2026-07-18)** — zwei allgemeine Codegen-Bugs bei
Closures/Lambdas + fehlende Felddeklaration. Stand: 58/61 → 59/61
(KNOWN_BROKEN 3 → 2).

**Codegen-Grundfix 1 — Capture eines by-value-Parameters.** Beim Bau des
Closure-Env lud der Codegen jede erfasste Variable per `load {ty}, {ty}*
%name`. Das stimmt für lokale Allocas, aber Funktionsparameter leben als
direkte SSA-Werte (`%handler`), nicht als Alloca — `load i64, i64* %handler`
auf einem i64-SSA-Wert = ungültiges IR. Fix: den Ident-Read spiegeln — ist die
erfasste Variable ein Param (`ctx.params`), den SSA-Wert `%name` direkt ins Env
speichern, sonst wie bisher aus dem Alloca laden. (`EventEmitter::once` fängt
`handler` in einem Wrapper-Lambda ein.)

**Codegen-Grundfix 2 — `return <void-Ausdruck>`.** Ein Lambda-Body `{ f(); }`,
dessen Schwanz ein void-Call ist, wurde als `Return(block)` emittiert und
erzeugte `ret void 0` (bzw. nach Teil-Fix `ret void` in einer i64-Funktion).
Lambdas nutzen eine uniforme i64-Return-ABI (Default ohne Annotation). Fix in
der Return-Codegen: (a) ist die Funktion selbst void → `ret void`; (b) ist der
Ausdruck void, die Funktion aber nicht → Dummy des Zieltyps zurückgeben
(`ret i64 0` / `ret <ptr> null`); dazu der Default-Terminator konsistent
(`ret void` nur bei void-Funktion, sonst `ret {ty} 0`).

**Modul-Fix:** `EventEmitter` bekam `var listeners: Map<String,
List<fnc(JsonValue) -> Nothing>>`.

Verifiziert: `on`/`emit`-Kette (2 Handler feuern), `once`-Wrapper mit Capture
feuert, `listenerCount` korrekt.

**Zusammenhang mit pool (Bug 27, offen):** verwandt, aber NICHT dasselbe — die
pool-`factory`-Lücke ist die Lambda-**als-Argument**-Repräsentation (nackter
fn-Zeiger statt Closure-Struct), die diese Fixes nicht berühren. Bleibt offen.

**Verbleibende KNOWN_BROKEN (2):** rest_framework (doppelter `entry:`-Block),
http2_server (Parser: verschachtelte Lvalue-Zuweisung `map[key].field = val`).

---

## Bug 31 — rest_framework: entry-Block-Kollision + Return(None) + Import + Lambda-Typ

**Status: GEFIXT (2026-07-18)** — vier Ursachen (zwei allgemeine Codegen-Bugs +
zwei Modul-Fixes). Stand: 59/61 → 60/61 (KNOWN_BROKEN 2 → 1).

**Codegen-Grundfix 1 — Basisblock `entry` kollidiert mit Param `entry`.** Eine
Methode `RestApi::wrapHandler(entry: RouteEntry)` erzeugte `define ...
@...(i64* %entry) { entry: ... }` — LLVM benennt das Label `entry:` implizit
`%entry`, was mit dem Param `%entry` kollidiert („unable to create block named
'entry'"). Nichts verzweigt je auf `%entry`, also den Entry-Block projektweit
in `entry.tnx` umbenannt (Punkt → kann kein Tinox-Identifier/Param je treffen).

**Codegen-Grundfix 2 — bare `return;` in non-void-Funktion.** `Return(None)`
emittierte immer `ret void`; in einem Lambda mit uniformer i64-Return-ABI
(mehrere frühe `return;` in `wrapHandler`s Wrapper) = ungültig. Jetzt am
Funktions-Rückgabetyp ausgerichtet: void → `ret void`, sonst Dummy
(`ret i64 0` / `ret <ptr> null`). Ergänzt Bug 30 (dort `Return(Some(void))`).

**Modul-Fix 1:** `import tinox.core.http_server` fehlte — `HttpServer::new`/
`.get`/`.delete` etc. waren Geister (`@HttpServer_delete` undefined).

**Modul-Fix 2:** der zurückgegebene Wrapper `return ctx => { … }` hatte einen
inferierten Param-Typ, der sich nicht auf `ctx.request.getHeader(…)` fortpflanzt
(Rückgabeposition hat keine Argument-basierte Lambda-Param-Inferenz →
`@getHeader` undefined). Auf die getypte Form `fnc(ctx: HttpContext) -> Nothing
{ … }` umgestellt (Argument-Position-Lambdas wie `server.use(ctx => …)` bleiben
über Argument-Inferenz grün).

Verifiziert: `GET::new("/x")`, `g.path == "/x"`.

**Verbleibende KNOWN_BROKEN (1):** http2_server (Parser: verschachtelte
Lvalue-Zuweisung `map[key].field = val`).

---

## Bug 32 — http2_server: verschachteltes Zuweisungsziel + Imports + Ghost — Bug 20 KOMPLETT

**Status: GEFIXT (2026-07-18)** — das letzte Modul. Stand: 60/61 → **61/61
grün, KNOWN_BROKEN leer**. Bug 20 (Stdlib-Großbefund, ursprünglich 42/61
kaputt) ist damit vollständig abgearbeitet.

**Parser-Fix (allgemein) — Zuweisung an Feld hinter Index-Kette.** Der
handgeschriebene Statement-Parser kannte `map[key].field = val` nicht: nach
dem Index-Ketten-Zweig (`obj.field[idx]...`) behandelte er nur `.method(...)`
und `.field`-Ketten, dann `expect(Semicolon)` — bei `= "closed"` also
„expected Semicolon, found Equals". Jetzt wird nach der Ketten-Schleife auch
`=` (Assignment) und Compound-Assign (`+=` etc.) auf das erreichte
FieldAccess-Ziel behandelt. Betraf `conn.streams[sid].state = "closed";`.

**Modul-Fixes:**
- `import tinox.core.hpack` (HpackDynTable/HpackHeader) und
  `import tinox.core.http_server` (Route/RouteMatcher/HttpContext/Http…)
  fehlten — `HpackDynTable_setMaxSize`, `Route_handler` waren dadurch Geister
  (die Methoden/Felder existieren, nur unimportiert). `route.handler(ctx)`
  ist ein fn-Feld-Aufruf auf dem deklarierten `Route.handler`-Feld.
- Echter Ghost-Builtin `httpServerReadRawBytes(fd, count) -> String` (Roh-
  Bytes von einem Socket-fd fürs HTTP/2-Framing) in `runtime.c` ergänzt
  (via `read(2)`, modelliert nach `socketReceive`) und per `extern fn` im
  Modul deklariert.

Verifiziert: `Http2FrameType::HEADERS()` == 1, Modul kompiliert & Smoke grün.

**Bug 20 Endstand:** 61/61 Stdlib-Module grün. Unterwegs entstandene
allgemeine Compiler-Fixes (Auswahl): Generics-Instanzmethoden (21),
Ghost-Builtins-Runden (23/24/25), Ternär-Typ-Erhalt + String→Zahl-Casts (26),
generische Basis-Layouts + Container-Marker + self-Verschiebung + rekursive
Typparameter-Erasure (27), `== null` für Referenzen (28), Closure-Param-
Capture + void-Returns (30), Entry-Block-Kollision + `return;`-Coercion (31),
verschachteltes Lvalue im Parser (32).

---

## Bug 33 — Closure-Repräsentation vereinheitlicht: pool.factory nutzbar

**Status: GEFIXT (2026-07-18)** — die in Bug 27/30 dokumentierte offene
Closure-Lücke. `pool`s `newWithFactory`/`acquire`-über-`factory`-Pfad
funktioniert jetzt (Callback-Feld auf einer generischen Klasse).

**Codegen-Fix 1 — Argumentliste beim indirekten Closure-Aufruf.** Alle
Closure-Call-Stellen (lokale fnc-Werte, `arr[i](...)`, fn-Felder) bauten die
Argliste als `"{args}, i64* {env}"`. Bei einem 0-Argument-Closure war `args`
leer → `call i64 %fp(, i64* %env)` (führendes Komma) = ungültiges IR. Neuer
Helper `closure_call_args` fügt das Komma nur bei nicht-leeren Args ein; die
`is_local_fn`-Zweige bekamen zusätzlich fehlende void-Rückgabe-Behandlung.

**Codegen-Fix 2 — generische Klassenmethoden mit `fnc`-Parametern werden
emittiert.** Die Spezialisierung übersprang Methoden mit einem `fnc`-Parameter
komplett (alter Punt aus der Zeit vor der einheitlichen Closure-Darstellung) —
`Pool::newWithFactory(f: fnc()->T)` landete dadurch im Enum-Variant-Fallback
und konstruierte Datenmüll (Feld 0 = Zeichensumme des Methodennamens, „940"/
„1470"). Da Methoden mit EIGENEN Typparametern (`fn map<U>(fnc(T)->U)`) schon
vorher übersprungen werden (`method.type_params`), sind die verbleibenden
`fnc`-Parameter voll konkret — die normale Signatur-Übersetzung (`fnc → i64`,
wie bei nicht-generischen Klassen) genügt. Skip entfernt.

Verifiziert: `Pool::newWithFactory(2, fnc()->Int64{return 7;})` →
`acquire()` == 7 (full cycle acquire/release/re-acquire == 42/42); minimaler
generischer Fall `Wrap::withMaker(fnc…)` == 55. pool-Smoke übt jetzt den
Factory-Pfad. `make check` voll grün.

**Damit ist auch die letzte bewusst offene Baustelle aus dem Bug-20-Komplex
geschlossen.**

---

## Feature 34 — HTTPS/TLS für den HTTP-Server

**Status: IMPLEMENTIERT (2026-07-19)** — `HttpServer::listenTls(cert, key)` liefert
echtes HTTPS (TLSv1.3 verifiziert). Vorher konnte der Server (und Client) nur
Klartext-`http://` (runtime.c sagte explizit „Plaintext http:// only, kein TLS").

**Kernidee — Connection-Handles statt roher fds.** Bei TLS reicht ein fd nicht,
weil jede Verbindung ein eigenes `SSL*` braucht. Neu: `TinoxConn {int fd; void* ssl}`
(GC-alloziert), dessen Zeiger als opakes `int64`-Handle zurückgegeben wird
(Userspace-Adresse ist stets > 0, Fehler = -1). `ssl==NULL` = Plaintext — damit
teilen sich http und https **denselben** Lese-/Schreib-Pfad über `conn_recv`/
`conn_send`/`conn_close`, die intern auf `SSL_read/Write` vs. `recv/send`
verzweigen. `httpServerReadRequest` wurde auf den geteilten Kern
`conn_read_request(TinoxConn*)` refaktoriert (Content-Length-Logik unverändert).

**Neue Runtime-Funktionen (runtime.c):**
- `httpServerCreateTls(port, certPath, keyPath)` — lädt Cert-Chain + Key (PEM),
  prüft Key-Paarung, bindet/lauscht wie `httpServerCreate`; -1 bei Fehler.
- `httpServerAcceptTls(serverFd)` — accept + blockierender `SSL_accept`-Handshake,
  liefert Conn-Handle.
- `httpServerAcceptConnHandle(serverFd)` — Plaintext-accept, das ebenfalls ein
  Conn-Handle liefert (damit `listen()` denselben Loop nutzt).
- `httpConnReadRequest/SendRaw/Close(conn)` — I/O über das Handle.

Alle in typecheck (`symbols.functions`) + codegen (`declare`) registriert.

**`.tnx`-Seite (http_server.tnx):** `handleRequest` bekommt jetzt ein Conn-Handle
statt eines fds und nutzt `httpConn*`; `listen()` (Plaintext) und das neue
`listenTls()` teilen sich diesen Handler — der einzige Unterschied ist
`httpServerAcceptConnHandle` vs. `httpServerAcceptTls`.

**Build — opt-in per `TINOX_TLS=1`** (main.rs): setzt `-DTINOX_TLS` beim
Runtime-Compile und linkt `-lssl -lcrypto`. Default-Build bleibt bewusst
OpenSSL-frei (Zeile-1651-Design). Ohne das Flag liefern die `*Tls`-Funktionen -1
mit klarer stderr-Diagnose statt eines Linkfehlers.

**Verifiziert:** self-signed Cert, `curl -k https://localhost:8443/hello` → 200
„Hallo ueber TLS!", 404-Routing, `openssl s_client` handelt TLSv1.3 /
AES-256-GCM aus; Plain-HTTP gegen den TLS-Port wird korrekt abgewiesen
(`tls_validate_record_header:http request`). Default-Build ohne Flag: Plaintext
`listen()` unverändert grün, kein OpenSSL gelinkt; `listenTls` bricht sauber ab
(kein Crash). `make check` voll grün.

**Bewusste Nicht-Ziele (v2):** SNI/mehrere Zerts, mTLS/Client-Zertifikate,
ALPN/HTTP2-über-TLS, non-blocking Handshake. Außerdem: die schnelle
route-basierte C-epoll-Loop (`tinox_HttpServer_listen`) bleibt Plaintext — TLS
dort erfordert per-fd-`SSL*`-Mapping + Handshake im level-triggered epoll und
ist ein separater Umbau. Der `.tnx`-`HttpServer` (der `listenTls` nutzt) ist der
gelieferte Pfad.

---

## Bug 35 — Uncaught `throw` wird still geschluckt (Programm läuft weiter, exit 0)

**Status: PRIMÄR GEFIXT (2026-07-19); Restschwäche in Bug 40 geschlossen.** Ein
uncaught `throw` ist jetzt laut und
fatal: `main` (runtime.c) prüft nach `tinox_main()` den globalen Slot
`__tinox_err` — ist er != 0, wurde nirgends gefangen → `fprintf(stderr,
"Uncaught error: %s", …)` + Exit 1. Ein gefangener throw setzt den Slot via
`emit_global_err_check` auf 0 zurück, löst also keine Falschmeldung aus.
Verifiziert: Repro unten meldet jetzt `Uncaught error: geworfen` und Exit 1;
`try`/`catch`-Fälle unverändert grün (Exit 0). `make check` grün.

**Bewusst noch offen (tiefere Design-Schwäche, s.u.):** der throw macht weiterhin
einen *stillen Funktions-Return* mit Default-Wert, statt sofort zu unwinden — der
Code zwischen throw und Programmende läuft also noch mit Default-Werten durch
(im Repro erscheint `nach go` vor dem Abbruch). Der Fix garantiert nur, dass ein
uncaught throw das Programm am Ende nicht mit Erfolg (Exit 0) verlässt. Echtes
sofortiges Unwinding wäre der v2-Fix (setjmp/longjmp oder Result-Rückgabe-ABI).

---

**Ursprünglicher Befund (Status vor dem Fix):** Ein `throw` ohne umschließendes `try`
irgendwo in der Aufrufkette (inkl. `main`) beendet das Programm **nicht** und
meldet **nichts** — die werfende Funktion liefert stillschweigend einen
Default-Wert (0/null/void) und der Aufrufer läuft weiter, als wäre nichts
gewesen. Exit-Code bleibt 0.

**Reproduktion (minimal):**
```tnx
class Foo {
    var x: Int64;
    fn new() -> Foo { return Foo { x: 0 }; }
    fn go() -> Nothing {
        this.x = 0 - 1;
        if this.x < 0 { throw "geworfen"; }   // feuert
        println("kein throw");                  // wird korrekt übersprungen
    }
}
fn main() -> Int32 {
    let f: Foo = Foo::new();
    f.go();
    println("nach go");   // WIRD gedruckt — throw verpufft
    return 0;             // exit 0
}
```
Ausgabe: nur `nach go`, exit 0. Erwartet: Programm bricht mit der Fehlermeldung
`geworfen` und Exit != 0 ab.

**Ursache (codegen.rs).** Die Fehlerpropagierung läuft über einen globalen Slot
`@__tinox_err` plus Default-Return: `StmtKind::Throw` ohne `ctx.error_catch`
(~Z. 3991) parkt den Wert in `@__tinox_err` und macht `ret <default>`. Konsumiert
wird der Slot ausschließlich in `gen_try_stmt` (~Z. 8431): **nach jedem Statement
im `try`-Body** wird `emit_global_err_check` emittiert, das `@__tinox_err` lädt
und bei != 0 in den `catch`-Block springt. Es gibt **keine** solche Prüfung
außerhalb eines `try`. Fehlt also ein `try` in der gesamten Kette hoch bis `main`,
prüft niemand den Slot → der Fehler verschwindet, das Programm endet regulär mit 0.

**Verifiziert korrekt:** Mit umschließendem `try`/`catch` funktioniert alles
sauber, auch über Funktionsgrenzen (`fn risky(){throw "x";}` in `try{risky();}
catch e:String{…}` fängt, überspringt Folgecode, exit 0). Der Bug betrifft
**ausschließlich den uncaught-Fall**.

**Umgesetzter Fix (runtime.c `main`).** Nach `tinox_main()` wird der extern
sichtbare Slot `__tinox_err` geprüft; bei != 0 → `fprintf(stderr, "Uncaught
error: %s", (char*)err)` + `return 1`. Damit sind uncaught throws laut+fatal
statt still. (Der Wert ist typgeprüft String-oder-Error; als String gedruckt —
der Normalfall.) **Bekannte Zusatzschwäche desselben Designs (noch offen):** der
Slot wird nur an `try`-Body-Statement-Grenzen konsumiert und der throw returned
still mit Default — ein `throw` in einer Zwischenfunktion propagiert nur, weil
deren Aufruf zufällig als Statement in einem `try`-Body steht; dazwischenliegende
Frames laufen vorher noch mit Default-Rückgabewerten zu Ende (verzögerte/
unpräzise Propagierung). Ein robuster v2-Fix würde `@__tinox_err` auch nach jedem
potenziell werfenden Call außerhalb von `try` prüfen oder auf echtes Unwinding
via setjmp/longjmp bzw. Result-Rückgabe-ABI umstellen.

**Gefunden bei:** Feature 34 (HTTPS) — `HttpServer::listenTls` wirft bei
fehlgeschlagenem TLS-Setup; der Diagnose-Kanal ist dort deshalb bewusst die
C-stderr-Meldung, nicht der `throw`.

---

## Bug 36 — Failure-Mode-Härtung: `Class::method` auf unbekanntem Namen → harter Fehler

**Status: GEFIXT (2026-07-19).** Der „Kardinalfehler" aus der Sprach-Bewertung:
ein statischer Aufruf `Foo::bar(...)` auf einer Klasse/einem Enum, das **weder
importiert noch definiert** ist, erzeugte still Datenmüll statt eines
Compile-Fehlers. `Strings::trim("x")` ohne `import tinox.core.string` gab ein
Müll-Byte zurück, `Bogus::doStuff(42)` einen Zeigerwert — beide exit 0.

**Ursache (typecheck).** `Class::method(args)` und `Enum::Variant(args)` teilen
denselben AST-Knoten `ExprKind::EnumValue`. Löste der Name nicht als registrierte
Statik/Instanz-Methode auf (`enum_name_variant` in `symbols.functions`), fiel der
Code auf `return ValueType::Any` (mit Args) bzw. `Named(enum_name)` (ohne) zurück
— **kein Fehler**. Die Codegen baute daraus einen Enum-Variant-Fallback (Feld 0 =
Zeichensumme des Methodennamens) oder eine Any-Zelle.

**Fix.** In der `EnumValue`-Behandlung: ist `enum_name` **weder** ein bekanntes
Enum (`self.enums`), **noch** eine bekannte Klasse (`known_class_names`), **noch**
ein Typ-Parameter im Scope (`type_param_scope`, für `T::fromJson()` in Generics),
→ neuer harter `TypeError::UnresolvedStaticPath` („unresolved 'X::y': no type,
enum, or static method named 'X' in scope (missing import?)"). Bekannte Klasse
ohne registrierte Signatur und Typ-Parameter bleiben bewusst permissiv (Any), um
False Positives auf generische Statik zu vermeiden.

**Dabei aufgedeckt — Registrierungslücke: Enums in Namespaces.** Fast die gesamte
Stdlib liegt in `namespace tinox.core.X { … }`. `register_declarations` behandelte
im Namespace-Zweig `Class`/`Immutable`/`Function`, aber **nicht `Enum`** — d.h.
namespaced Enums landeten nie in `self.enums`. `Enum::Variant` daraus fiel bisher
still auf `Named(...)`. Nach der Härtung wurde das ein Fehler (`HttpStatus::…` in
rest, `MediaType::None` in rest_framework). Gefixt durch Spiegeln der
Top-Level-Enum-Registrierung in den Namespace-Zweig (enums + Payloads + Varianten-
Variablen). **Das ist ein allgemeiner Korrektheitsgewinn** (Match-Exhaustiveness,
Varianten-Typisierung über alle namespaced Enums).

**Dabei aufgedeckte latente Missing-Import-Bugs (vorher still Datenmüll):**
- `crypto.tnx`: `Random::nextInt(256)` zur **Schlüssel-/Zufallsbyte-Erzeugung**
  ohne `import tinox.core.random` — sicherheitsrelevant! (+import random)
- `http_server.tnx`: `Json::parse/serialize/deserialize` ohne import (+import json)
- `rest.tnx`: `Base64::encode` + `Json::parse` ohne imports (+import base64, json)
- `iter.tnx`: `Pair::new` (aus collections) ohne import (+import collections)
- `toml.tnx`: `Strings::trim` ohne import (+import string)
- `bitmap.tnx`: `Mathf::abs` ohne import (+import mathf)
- `fmt.tnx`: `Format::intToHex/intToBinary` ohne import (+import format)

**Verifiziert:** `Strings::trim`/`Bogus::doStuff` ohne Import → Compile-Fehler mit
Import-Hinweis; korrekt importiert → grün; Enum-mit-Payload, nackte Variante und
generische Statik (`Pool::newWithFactory`, `T::fromJson`) ohne False Positive;
`make check` voll grün.

**Damit ist der wichtigste Punkt aus der Sprach-Bewertung adressiert: silent
garbage → hard error.** Verbleibend (v2): auch „bekannte Klasse, aber Methode/
Signatur nicht gefunden" hart machen (aktuell permissiv wegen generischer Statik).

---

## Bug 37 — Failure-Mode-Härtung: Feldzugriff auf nicht deklariertes Feld → harter Fehler

**Status: GEFIXT (2026-07-19).** Fortsetzung der Härtung aus Bug 36, andere
Spielart desselben „silent garbage": eine **nicht-generische** Klasse **ganz ohne
Felddeklarationen** (Felder nur über `Class { feld: … }`-Literale benutzt) ließ
jeden Feldzugriff still auf `i64` defaulten. `Point` ohne `var x/y: Float64` →
`p.x + p.y` rechnete Integer-Mathe auf Float-Bits → `-9214364837600034816` statt
`7.5`, exit 0.

**Ursache (typecheck, `ExprKind::FieldAccess`).** Der Guard meldete `FieldNotFound`
nur, wenn die Klasse **mindestens ein** registriertes Feld hatte (Kommentar:
„generic classes using struct-literal fields won't have registered fields"). Eine
Klasse mit **null** deklarierten Feldern fiel komplett durch → `Any` im Typecheck,
`i64` in der Codegen. Der Guard war zu grob: er sollte nur generische Klassen
schonen, schonte aber auch schlicht unvollständig deklarierte.

**Fix.** Neues Register `generic_class_names` (befüllt bei der Klassenregistrierung
aus `c.type_params`). In `FieldAccess` wird jetzt gemeldet, wenn die Klasse
**mindestens ein Feld hat** (wie bisher) **ODER** eine **bekannte, nicht-generische**
Klasse ist (`known_class_names && !generic`). Generische Klassen und unbekannte
Named-Typen (Enum-Payloads etc.) bleiben permissiv → keine False Positives.

**Dabei aufgedeckte latente „i64-Garbage"-Klassen (Felddeklarationen ergänzt):**
- `bitmap.tnx` Bitmap: `width/height: Int64`, `pixels: List<Int64>`
- `cron.tnx` CronScheduler: `jobs: List<CronJob>`, `running: Bool`
- `ini.tnx` IniConfig: `sections: Map<String, Map<String, String>>`
- `semaphore.tnx` Semaphore: `count: Int64`, `waiting: List<Int64>`;
  Mutex: `locked: Bool`, `queue: List<Int64>`; RWLock: `readers/writers: Int64`
- `trie.tnx` Trie: `root: TrieNode`

**Verifiziert:** `Point` ohne Deklaration → `type Point has no field 'x'`; mit
`var x/y: Float64` → `7.5`; generische `Box<T>` ohne Felddeklaration erzeugt
**keinen** neuen Typfehler (permissiv). `make check` voll grün.

**Separater vorbestehender Fund (NICHT hier gefixt):** eine generische Klasse mit
einem Feld (`Box<T> { var value: T }`) segfaultet zur Laufzeit beim Zugriff auf
`this.value` — reiner Codegen-Bug im generischen Instanz-Layout, unabhängig von
dieser Typecheck-Härtung (mit `git stash` gegengeprüft: Segfault auch vor der
Änderung). Eigener Bug für später.

---

## Bug 38 — `this` in via `::` aufgerufener Instanzmethode las null-self → Segfault

**Status: GEFIXT (2026-07-19).** Der in Bug 37 als „separater vorbestehender Fund"
notierte Segfault. Betraf NICHT nur generische Klassen — trat bei jeder
Instanzmethode auf, die `this` benutzt und via `Class::method(obj)` (statt
Dot-Syntax `obj.method()`) aufgerufen wird.

**Symptom.** `fn get() -> Int64 { return this.value; }`, aufgerufen als
`IntBox::get(b)`, segfaultete. Dot-Syntax `b.get()` funktionierte dagegen.

**Ursache (codegen, Static-Dispatch).** IR-Diff war eindeutig:
- Definition: `@IntBox_get(i64* %self)` — self als (einziger) Parameter.
- Aufruf `::`: `@IntBox_get(i64* null, i64* %b)` — null-self vorangestellt PLUS
  das Objekt als zweites Arg. `%self` erhielt `null`, `%b` verpuffte als
  überzähliges Arg → `this.value` = `load ptr null` → Segfault.

`emit_static_dispatch_call` (und der generische Receiver-Marker-Pfad) stellten
für Instanzmethoden (`fn`, nicht `fnc`) *immer* ein `i64* null`-self voran. Das
passt zur Stdlib-Konvention „Objekt als expliziter erster *deklarierter* Param"
(`fn getString(config: IniConfig, …)`, self ungenutzt), bricht aber die
`this`-basierte Variante, bei der das Objekt das self IST.

**Fix — Disambiguierung über die Arg-Zahl.** `method_param_types` kennt die Zahl
der *deklarierten* Params (ohne self). Beim `::`-Aufruf einer Instanzmethode:
- `args == declared` → Objekt nicht als self übergeben (oder als expliziter erster
  Param) → null-self voranstellen (unverändert).
- `args == declared + 1` → das führende Arg IST das Empfänger-Objekt (self) →
  KEIN null-self; das Objekt wird zum self-Parameter.

Beide Stile funktionieren damit: `IniConfig::getString(c,"s","k","?")` (4 decl, 4
args → null-self, `config`=c) und `IntBox::get(b)` (0 decl, 1 arg → self=b). An
beiden Emissionsstellen (nicht-generisch + generischer Pfad) angewandt.

**Verifiziert:** e2e-Regressionstest `tests/e2e/this_via_static_dispatch.tnx`
(nicht-generisch, generisch, obj+Param) → 42/5/8/7/99; `make check` voll grün
(keine Stdlib-Regression trotz massiver Nutzung der expliziten-Objekt-Konvention).

**Separat noch offen:** `.toString()` auf einem Wert von generischem Rückgabetyp
`T` (`Box<T>::setAndGet(...).toString()`) emittiert ein unaufgelöstes `@toString`
statt `@tinox_int_to_string` — vorbestehende generische Dispatch-Lücke, mit
`git stash` gegengeprüft, unabhängig von diesem Fix.

---

## Bug 39 — generische Spezialisierungswahl: Objekt-Arg verschob T-Bindung → falsche Zeiger-Variante

**Status: GEFIXT (2026-07-19).** Der in Bug 38 notierte separate `@toString`-Fund.
Direktes `Box::setAndGet(b, 99).toString()` (generische Instanzmethode mit
T-Param, Rückgabewert direkt weiterverwendet) erzeugte ungültiges IR
(`use of undefined value '@toString'`); mit typannotierter Zwischenvariable
(`let r: Int64 = …`) ging es.

**Ursache (codegen, generische Bindungsinferenz).** Das IR zeigte, dass der Aufruf
die **Zeiger**-Spezialisierung `Box__i64P_setAndGet` (Rückgabe `i64*`) statt der
Wert-Variante `Box__i64` wählte — und `.toString()` auf `i64*` ist unauflösbar.
Grund: die Bindungsinferenz matcht `method.params[pi]` gegen `arg_vals[pi]`,
ignorierte aber das **führende Objekt-Arg** aus der `Class::method(obj, …)`-
Konvention (Bug 38). Bei `setAndGet(v: T)` wurde `v` (pi=0) gegen `arg_vals[0]` =
das Objekt `b` (LLVM-Typ `i64*`) gebunden statt gegen `99` (`i64`) → `T = i64P`.

**Fix.** `arg_offset = if arg_vals.len() == method.params.len() + 1 { 1 } else { 0 }`
(genau die Bug-38-Disambiguierung: führt der Aufruf das Objekt als erstes Arg?).
Die Inferenz liest jetzt `arg_vals[pi + arg_offset]` / `args[pi + arg_offset]` —
so wird `v` gegen `99` gebunden → `T = i64` → richtige Wert-Spezialisierung. Der
Empfänger-Stil (`Cache::set(cache, …)`, Objekt IST deklarierter Param,
`args == declared` → offset 0) bleibt unberührt.

**Verifiziert:** `Box::setAndGet(b, 99).toString()` → 99, `Box<String>` → "world";
Regressionsfall im e2e-Test `this_via_static_dispatch.tnx` ergänzt (→ 123);
`make check` voll grün (Cache/Option/Result/collections nutzen den
Empfänger-Stil ausgiebig — keine Regression).

---

## Bug 40 — Echtes throw-Unwinding: sofortige Propagierung statt bis zur nächsten try-Grenze

**Status: GEFIXT (2026-07-19).** Schließt die in Bug 35 bewusst offen gelassene
tiefere Design-Schwäche. Vorher stoppte ein `throw` nur seine eigene Funktion
(via `ret <default>` + globalem Slot `@__tinox_err`); **Zwischen-Frames ohne try
und Schleifen liefen mit Default-Werten weiter**, bis ein `try` an einer
Statement-Grenze den Slot prüfte. Jetzt wird ein geworfener Fehler **sofort** (auf
Statement-Granularität) durch alle Frames und aus Schleifen heraus propagiert.

**Ansatz (bewusst NICHT setjmp/longjmp).** setjmp/longjmp wäre architektonisch
sauber (Prototyp durch `opt -O3 + llc -O3` verifiziert), erfordert aber
Handler-Stack-Aufräumen bei `return`/`break`/`continue`/`finally`/`defer` aus
einem try — große, fehleranfällige Blast-Radius. Stattdessen wird der bewährte
`@__tinox_err`-Flag-Mechanismus wiederverwendet: **nach jedem Statement, das
werfen kann, eine Propagierungs-Prüfung** (`emit_post_stmt_throw_check`) im
Block-Handler (und am try-Body-Ende):
- innerhalb eines try → Fehler konsumieren, zum catch springen (wie bisher);
- sonst → `ret <default>`, Flag bleibt gesetzt, sodass die Prüfung im aufrufenden
  Frame (oder der Runtime-Entry-Check aus Bug 35) weiter propagiert.

`throw` selbst unverändert (Slot setzen + Default-Return); `try` und der
Runtime-main-Check unverändert. Neu nur die per-Statement-Prüfung, gegated durch
`stmt_may_throw` (konservativer Syntax-Walker: nur nach Statements mit Call/`throw`
— reine Arithmetik/Zuweisungen bleiben ungeprüft, kein Overhead) und
`last_is_terminator` (keine Prüfung nach `ret`/`br`, sonst ungültiges IR).

**Granularität:** Statement-Ebene. Ein throw in einer Zwischen-Funktion stoppt den
Aufrufer beim nächsten Statement (nicht mitten in einem mehr-Call-Ausdruck wie
`a() + b()` — dort liefe `b()` nach `a()`s throw noch, dann greift die Prüfung).
Für die Praxis (Zwischen-Frames, Schleifen, Rückgabewerte) ist das vollständiges
sofortiges Unwinding.

**Verifiziert:** Bug-35-Repro druckt „nach go" jetzt NICHT (sofortiger Abbruch);
Schleife mit werfendem Body stoppt sofort; `try`/`catch` fängt aus Schleifen und
über mehrere Frames; Rückgabewert-Funktion propagiert. e2e-Regressionstest
`tests/e2e/throw_unwinding.tnx`; `make check` voll grün (der per-Statement-Check
betrifft ALLEN Block-Code — Dogfood inkl. jgrep-tinox/Benchmarks unverändert grün).

**Bewusst noch offen (v3):** Sub-Statement-Granularität (perfekte Immediacy in
`a()+b()`) bräuchte Post-Call-Checks oder setjmp; `try`-`finally` ohne `catch`
schluckt weiterhin (kein Re-throw, wie im Alt-Mechanismus).

**Perf-Nachtrag (2026-07-20):** gemessener Overhead des per-Statement-Checks —
call-freie heiße Schleifen 0 % (`stmt_may_throw`=false); inlinebare Calls 0 %
(`opt -O3` eliminiert den Check komplett); nur nicht-inlinebare Call-*Statements*
in extrem heißem Code ~17 % (worst case: rekursives `compute(40)`, 160→186 ms).
Der Overhead ist mit der throw-Effekt-Analyse (Bug 48) eliminiert.

---

## Bug 41 — `defer` lief nicht beim throw-Unwinding (Ressourcen-Leak auf Fehlerpfaden)

**Status: GEFIXT (2026-07-19).** `defer`-Blöcke (Cleanup-Mechanismus wie in Go)
liefen bei normalem `return`, wurden aber bei einem `throw` **still übersprungen**
— genau dann, wenn Cleanup am wichtigsten ist. Dateien/Locks/Connections blieben
auf Fehlerpfaden offen. Durch das jetzt sofortige Unwinding (Bug 40) noch
prominenter.

**Ursache.** Das alte IR emittierte die deferred Statements des Blocks *nach* dem
throw-`ret` (als toter Code hinter dem Terminator, den `opt`/`llc` verwerfen) —
sie liefen also nie. `emit_ret_default` (throw ohne umschließenden try) und der
Bug-40-Propagate-Check gaben ohne Cleanup zurück.

**Fix.** Neuer Helper `emit_unwind_defers`, der VOR dem `ret` **alle aktiven
defer-Scopes** (innerster zuerst, LIFO) ausführt — aufgerufen im throw-Codegen
(non-catch-Pfad) und im `emit_post_stmt_throw_check` (Propagate-Pfad). Anders als
`gen_defer_scope` (nur innerster Scope, normaler Blockaustritt) muss ein
entweichender throw *jeden* umschließenden Scope aufräumen: ein throw in einer
Schleife muss auch den Funktions-`defer` ausführen. `in_defer_exec`-Guard
verhindert Rekursion; der defer_stack bleibt intakt, sodass der normale
(nicht-werfende) Pfad seine Scopes weiterhin beim jeweiligen Blockaustritt läuft.

**Verifiziert:** defer läuft bei uncaught-throw; Funktions-`defer` läuft bei throw
aus verschachteltem Loop (ALLE Scopes); mehrere defers in LIFO-Reihenfolge;
normaler `return`-Pfad unverändert. e2e-Regressionstest
`tests/e2e/defer_on_throw.tnx`; `make check` voll grün.

**Bewusst noch offen:** defer-Scopes zwischen throw und einem `catch` in
DERSELBEN Funktion (teilweises Unwinding innerhalb eines Frames) — der Fix deckt
das Entweichen aus dem Frame ab (der häufige Ressourcen-Fall: Funktion öffnet
Ressource, `defer close`, ruft etwas das wirft). Verwandt: [[Bug 40]].

---

## Bug 42 — `try`-`finally` ohne `catch`: kompilierte nicht + schluckte die Exception

**Status: GEFIXT (2026-07-19).** Zwei Fehler in einem: (1) `try { … } finally { … }`
**ohne** `catch` erzeugte ungültiges IR und kompilierte gar nicht — der
leere-catches-Zweig emittierte `catch_bb:` direkt gefolgt von einem weiteren
Label ohne Terminator dazwischen (`opt: expected instruction opcode`). Der Fall
wurde nie getestet (Tests hatten immer ein `catch`). (2) Selbst mit behobenem IR
wäre die Semantik falsch: ein `try`-`finally` ohne `catch` muss `finally`
ausführen und den Fehler dann **re-werfen** (propagieren), nicht schlucken.

**Fix (gen_try_stmt-Tail umgebaut).** Neuer Konvergenz-Block `try_converge`, durch
den normaler Pfad UND catch-Dispatch (via `finally`, falls vorhanden) laufen —
nie direkt zu `end_bb`. Am Konvergenzpunkt bei **leeren catches**: `error_var`
prüfen (0 auf Normalpfad, Fehlerwert auf Fehlerpfad) — bei != 0 re-werfen NACH
`finally`: an den umschließenden try dieser Funktion (`ctx.error_catch`) übergeben
oder aus dem Frame propagieren (`@__tinox_err` setzen, Unwind-Defers aus Bug 41,
Default-Return). Der leere-catches-Zweig bekam den fehlenden Terminator.

**Verifiziert:** `try`-`finally` ohne catch mit throw → `finally` läuft, Fehler
propagiert in den äußeren catch bzw. uncaught (exit 1); ohne Fehler → normaler
Fluss; `try`-`catch`-`finally` mit gefangenem Fehler → **kein** Re-throw
(unverändert). e2e-Regressionstest `tests/e2e/try_finally_rethrow.tnx`;
`make check` voll grün.

Damit ist die Exception-Semantik vollständig: uncaught→fatal (35), sofortiges
Unwinding (40), defer-Cleanup auf Fehlerpfaden (41), finally+re-throw (42).

---

## Bug 43 — `Class::method` auf bekannter Klasse ohne diese Methode → harter Fehler

**Status: GEFIXT (2026-07-19).** Schließt die in Bug 36 bewusst permissiv
gelassene Restlücke: ein statischer Aufruf `Foo::bar()` auf einer **bekannten**
Klasse, die keine Methode `bar` hat (Tippfehler oder falsche Klasse), gab still
`Any` zurück statt eines Fehlers — die Codegen baute daraus Datenmüll.

**Fix (typecheck, `EnumValue`-Zweig).** Löst `Class_method` nicht in
`symbols.functions` auf (dort registriert: eigene, geerbte und generische
Methoden) UND ist `Class` eine bekannte **nicht-generische** Klasse (kein
`generic_class_names`, kein `type_param_scope`) → neuer harter
`TypeError::UnknownStaticMethod` („type 'X' has no method 'Y'"). Generische
Klassen und Typ-Parameter bleiben permissiv (Methode kann sich erst nach
Monomorphisierung auflösen) — gleiche Abgrenzung wie beim Feld-Check (Bug 37).

**Dabei aufgedeckter latenter Bug:** `ini.tnx` `IniConfig::getInt` rief intern
`Ini::getString(...)` auf — `getString` ist aber eine Methode von `IniConfig`,
nicht `Ini` (falsche Klasse). `getInt` gab dadurch **Datenmüll** zurück (still
`Any`). Auf `IniConfig::getString` korrigiert; verifiziert: `getInt` für
`port=8080` liefert jetzt 8080 statt Müll.

**Verifiziert:** Tippfehler-Methode → Compile-Fehler; korrekte Methode → grün;
geerbte Methode via `::` löst NICHT falsch aus (Typecheck kennt den Kind-Schlüssel
— separater vorbestehender Codegen-Bug: geerbte Methoden werden nicht unter dem
mangled Kind-Namen emittiert, `@Derived_getN` undefined, mit `git stash`
gegengeprüft); generische Klasse bleibt permissiv. Typecheck-Unit-Tests
(`test_static_call_*`); `make check` voll grün.

---

## Bug 44 — geerbte Methode via `Class::method(obj)` erzeugte undefinierte Funktion

**Status: GEFIXT (2026-07-19).** Der in Bug 43 als separat notierte Fund.
`Derived::getN(d)` für eine von `Base` geerbte Methode rief `@Derived_getN` auf —
das aber nie emittiert wird (nur `@Base_getN` existiert) → ungültiges IR
(`use of undefined value '@Derived_getN'`). Die Dot-Syntax `d.getN()` funktionierte
dagegen.

**Ursache.** Der Codegen hat eine Map `method_impl: ClassName_method →
OwnerClassName_method` (löst Vererbung auf, `Derived_getN → Base_getN`). Der
Dot-Syntax-Pfad (MethodCall) nutzt sie; der `::`-Static-Dispatch
(`emit_static_dispatch_call`) nutzte sie NICHT und emittierte den nackten
`Derived_getN`-Aufruf.

**Fix.** In `emit_static_dispatch_call` den Schlüssel zuerst über `method_impl`
zum definierenden Vorfahren auflösen (wie der Dot-Pfad). Eigene Methoden mappen
auf sich selbst (Z. 1158), überschriebene auf die Kind-Version — daher bleibt
Override korrekt. Zentral in `emit_static_dispatch_call`, also profitiert auch
der generische Pfad.

**Verifiziert:** geerbte Methode ohne/mit Param via `::` → 7 / 15; überschriebene
Methode → Kind-Version („derived"). e2e-Regressionstest
`tests/e2e/inherited_static_dispatch.tnx`; `make check` voll grün.

---

## Bug 45 — nicht existente Enum-Variante `Enum::Variant` wurde still akzeptiert

**Status: GEFIXT (2026-07-19).** Analog zu Bug 43, aber für Enums: `Color::Purple`
auf einem Enum ohne `Purple`-Variante gab still `Named(Color)` zurück und baute
einen Bogus-Wert, statt zu fehlern.

**Fix (typecheck, `EnumValue`-Zweig).** Ist `enum_name` ein bekanntes Enum, wird
`variant` gegen die registrierte Variantenliste geprüft; fehlt sie → neuer harter
`TypeError::UnknownEnumVariant` („enum 'X' has no variant 'Y'").

**Namenskollisions-Falle (dabei entdeckt + gelöst).** Enum-Namen sind NICHT
modul-qualifiziert: `MediaType` ist in http_server (`APPLICATION_JSON`/
`PLAIN_TEXT`), rest (`ApplicationJson`/…) UND rest_framework (`None`/`Json`/…)
mit UNTERSCHIEDLICHEN Varianten definiert. Die flache `enums`-Map behielt bei
`insert` nur eine — das gültige `MediaType::None` (rest_framework) schlug dann
fehl, weil http_servers Version gewann. Fix: neuer Helper
`register_enum_variants` VEREINIGT die Varianten gleichnamiger Enums (statt
overwrite). Sicher, weil `self.enums` nur für `is_known_enum` (contains_key) und
diese Varianten-Prüfung genutzt wird (NICHT für Match-Exhaustiveness). Ein echter
Tippfehler (Variante in KEINER Definition) fällt weiter durch.

**Verifiziert:** `Color::Purple`/`Shape::Triangle(3.0)` → Compile-Fehler; gültige
Varianten (auch mit Payload) → grün; `MediaType::None` (Kollision) → grün;
`M::A`+`M::D` über zwei gleichnamige Enums → grün (Union). Typecheck-Unit-Tests
(`test_enum_*`); `make check` voll grün.

**Bekannte Grenze (v2):** Arg-Anzahl/-Typen einer Enum-Variante werden hier nicht
geprüft (nur der Name); die modul-übergreifende Enum-Namenskollision selbst
bleibt bestehen (Union maskiert sie nur für die Namensprüfung).

---

## Bug 46 — `Class::method(...)` prüfte die Argument-Anzahl nicht

**Status: GEFIXT (2026-07-19).** Freie Funktionen wurden auf Arg-Anzahl geprüft
(`expected N arguments, found M`), `::`-Methodenaufrufe aber nicht: `Calc::add(c)`
(fehlende Args) gab Datenmüll, `Mathy::square(3,4,5)` (statisch, zu viele)
ignorierte still die Extra-Args. Loose i64-ABI → keine Laufzeit-Fehler.

**Fix (typecheck, `EnumValue`-static_key-Zweig).** Nach dem Auflösen der Signatur
wird die Arg-Anzahl geprüft. Wegen der Self-Konvention-Ambiguität (Bug 38) zwei
Fälle:
- Instanzmethode (`fn`, führender synthetischer `"self"`-Param): das Empfänger-
  Objekt darf als führendes Arg (`args == declared+1`) ODER weggelassen/als
  expliziter erster deklarierter Param (`args == declared`) übergeben werden —
  beide akzeptiert (fängt grobe Fehler wie fehlende/zu viele Args).
- Statische Methode (`fnc`, kein `self`): `args == declared` exakt.

**Verifiziert:** Instanz zu wenig Args → Fehler; statisch falsche Anzahl → Fehler;
beide legitimen Instanz-Stile (obj-als-self `Calc::add(c,3,4)`, expliziter
Objekt-Param `Store::getWith(s,5)`) → grün. Typecheck-Unit-Tests
(`test_static_call_*_args*`); `make check` voll grün — **keine False Positives**,
alle Stdlib-`::`-Aufrufe haben korrekte Arg-Zahlen (kein latenter Bug diesmal,
aber die Prüfung schützt künftig vor dieser Fehlerklasse).

**Grenze:** Instanzmethoden-Prüfung ist wegen der dualen Konvention permissiv
(akzeptiert `declared` und `declared+1`) — ein subtiler Off-by-one mit passendem
Typ (`c` als erstes Int-Arg) bleibt unerkannt; das bräuchte präzise
Argument-TYP-Prüfung durch die Self-Konvention hindurch (v2). Enum-Varianten-Args
(Bug 45) sind weiterhin ungeprüft.

---

## Bug 47 — Self-Konvention-Sondierung + sichere Verschärfung der Arg-Prüfung

**Status: TEILWEISE (2026-07-19).** Anlauf, die duale Self-Konvention (Bug 38) an
der Wurzel zu entschärfen, statt nur Symptome. **Ergebnis: die Ambiguität ist
NICHT statisch auflösbar** — die permissive Arg-Prüfung (Bug 46) ist das korrekte
lokale Optimum. Eine sichere Teilverschärfung ist aber möglich und umgesetzt.

**Sondierung der Stdlib (798 Instanzmethoden):** nur 198 (25 %) nutzen `this`; 75 %
übergeben den Empfänger als expliziten ersten Param (Style B, inkl. generischer)
oder sind Factories/Namespace-Helfer (`Hex::encode`). Eine echte Vereinheitlichung
auf EINE Konvention hieße ~150+ Methoden umzuschreiben — hohes Risiko, geringer
Nutzen (das Codegen-ABI funktioniert über die Arg-Zahl-Heuristik bereits korrekt).

**Warum exakt nicht geht (empirisch bewiesen):** `Class::method(obj)` bindet obj
als self — auch bei einer Methode, die `this` NICHT benutzt (`fn label() { return
"x"; }`, mit obj-als-self das ignoriert wird). Und ein reiner Namespace-Helfer
(`Hex::encode(data)`) hat gar keinen Empfänger. Beide sind `this`-los, brauchen
aber unterschiedliche Arg-Zahlen — nicht unterscheidbar. Ein voll-exakter Check
erzeugte prompt einen False Positive auf `Derived::label(d)`.

**Sichere Verschärfung (umgesetzt).** Neuer `this`-Scanner + Register
`method_uses_this` (an allen 3 Registrierungsstellen inkl. Vererbung). Eine
Methode, die `this` BENUTZT, braucht den Empfänger zwingend → dort ist die
Arg-Zahl exakt `declared+1` (fängt `Class::m()` ohne Objekt, das sonst zur
Laufzeit einen null-self dereferenziert). Alle anderen (this-los) bleiben
permissiv (`declared`/`declared+1`). Static (`fnc`): exakt `declared`.

**Verifiziert:** `Box::getN()` ohne Objekt (getN nutzt this) → Fehler;
`Derived::label(d)` (label this-los) → grün; `Hex::encode`/Style-B → grün;
geerbte this-Methode → grün (kein False Positive). Typecheck-Unit-Tests
(`test_static_call_this_method_*`, `*receiver_agnostic*`); `make check` voll grün.

**Fazit für die ABI-Wurzel:** Die echte Auflösung der Ambiguität erfordert die
ABI-/Konventions-Migration (eine Konvention) ODER die getypte Wertdarstellung
(Problem 1) — beides teuer. Bis dahin ist permissiv-mit-`this`-Verschärfung das
Optimum. Nächster sinnvoller Schritt ist NICHT weitere Konventions-Arbeit,
sondern getypte Struct-Layouts (B1).

---

## Bug 48 — throw-Effekt-Analyse: throw-Unwinding zero-cost-when-unused

**Status: GEFIXT (2026-07-20).** Beseitigt den in Bug 40 gemessenen worst-case-
Overhead (~17 %). `stmt_may_throw` war rein syntaktisch — es markierte JEDEN Call
als potenziell werfend und emittierte danach einen `@__tinox_err`-Check, auch nach
Funktionen, die nachweislich nie werfen (`compute()` in der Fibonacci-Rekursion).

**Fix — Call-Graph-Fixpunkt-Analyse** (`analyze_throw_effects`, läuft vor der
Body-Emission). Eine Funktion „kann werfen", wenn ihr Rumpf ein `throw` enthält
ODER eine Funktion aufruft, die werfen kann (transitiv). Zwei Ergebnis-Mengen:
`throwing_free_fns` (freie fn-Namen) und `throwing_method_basenames` (Methoden-
Basisnamen, über die keine Objekt-Typinfo nötig ist — `obj.m()`/`Class::m()`
werfen gdw. IRGENDEIN `m` werfen kann, sichere Über-Approximation). `stmt_may_throw`
konsultiert die Mengen: ein Call auf ein Ziel, das NICHT drin ist, zählt nicht als
werfend → kein Check.

**Korrektheit (kritisch).** Konservativ in die sichere Richtung: unauflösbare/
dynamische Calls (Lambda-Variable, `New`, `await`/`recv`/`spawn`) gelten als
werfend. Da die Analyse ALLE user-fns/Methoden (inkl. Namespaces rekursiv +
Interface-Methoden) erfasst, ist „nicht in der Menge → Builtin/nachweislich
non-throwing → kein throw" korrekt — ein echter throw wird nie übersehen, Bug 40
bleibt korrekt.

**Verifiziert:** `@pure` (rekursiv, wirft nie) → **0** Checks; `@a`/`@b`/`@mixed`
(rufen werfende Kette) → Checks erhalten; werfende Propagierung über 3 Frames +
gemischte fn (nicht-werfender + werfender Call) korrekt. worst-case-Benchmark
`compute(40)`: 186 → **156 ms** (Overhead weg). e2e-Regressionstest
`tests/e2e/throw_effect_analysis.tnx` (prüft beide Seiten); alle throw/defer/
try-finally-e2e-Tests unverändert grün; `make check` voll grün.

Damit ist throw-Unwinding **zero-cost when unused**: Programme ohne erreichbaren
`throw` bekommen gar keine Checks, nur Funktionen auf einem throw-Pfad zahlen.

---

## Bug 49 — Parser: Stack-Overflow-Crash bei tief verschachtelter Eingabe

**Status: GEFIXT (2026-07-20).** Der rekursive-Descent-Parser (und die
nachgelagerten AST-Walks) hatten kein Tiefenlimit → pathologisch tiefe Eingabe
crashte den Compiler (SIGABRT/Stack-Overflow) statt eines sauberen Fehlers —
klassische Parser-DoS. Betroffen: geklammerte Ausdrücke `(((…`, Array-Literale
`[[[…`, verschachtelte Blöcke `if{if{…`, Postfix-Ketten `a.a.a.…` (je ×50000).

**Sondierung:** verschiedene Konstrukte crashten bei verschiedenen Tiefen —
Parser-Rekursion (paren) erst >95, aber nested-Array-Literale schon bei ~43 (die
TEUERSTE Phase ist nicht der Parser, sondern typecheck/codegen der verschachtelten
Container). Ein einheitliches niedriges Parser-Limit hätte legitimen Code gebrochen.

**Fix — zwei Ebenen, wie in ausgereiften Compilern (rustc):**
1. **Großer Compiler-Stack.** `main` läuft den Compiler jetzt auf einem Thread mit
   512 MB Stack (`std::thread::Builder::stack_size`). Das verschiebt die sichere
   Verschachtelungstiefe ALLER Phasen (Parser, node-ids, typecheck, codegen) um
   ~64× nach oben (nested-Array-Crash von ~43 auf ~2750). Ein Panic im Worker →
   Exit 101 (kein Double-Panic-Rauschen).
2. **Parser-Tiefen-Guard.** Feld `depth` + `MAX_RECURSION_DEPTH = 1000`, geprüft in
   `parse_expr` und `parse_stmt` (rekursive Eintrittspunkte) → sauberer Fehler
   „expression/statement nesting too deep". Postfix-Ketten sind iterativ (erhöhen
   die Rekursion nicht, bauen aber gleich tiefe AST) → separater Ketten-Zähler in
   `parse_postfix_expr` („postfix chain too deep"). Das Limit ist mit dem 512-MB-
   Stack sicher und weit über jedem realen Programm.

**Verifiziert:** alle 5 Pathologie-Fälle (×50000) → sauberer Fehler statt Crash;
legitim tiefe Eingabe (300 verschachtelte Arrays, 40-fach geklammert, verschachtelte
if) → kompiliert; e2e-Regressionstest `tests/e2e/deep_nesting_guard.tnx`;
`make check` voll grün (kein Stdlib-Programm überschreitet das Limit).

**Separater vorbestehender Fund (Bug 50):** Method-Ketten haben EXPONENTIELLE
Compile-Zeit (Kette 15→971 ms, 20→2666 ms; mit `git stash` als vor der Änderung
bestätigt) — der Guard verhindert den Crash, aber nicht diesen Hänger; eigener Bug.

---

## Bug 50 — Method-Ketten: exponentielle Typecheck-Zeit

**Status: GEFIXT (2026-07-20).** Der in Bug 49 gefundene Hänger. Eine Method-Kette
`c.n().n()…` hatte O(2ⁿ) Typecheck-Zeit (Kette 20 → 1,4 s, 24 → 23 s, 50 →
Timeout). Vorbestehend (mit `git stash` bestätigt).

**Ursache (typecheck).** `infer_type` eines `MethodCall` inferiert den Empfänger
ZWEIMAL: direkt (`let obj_ty = self.infer_type(obj)`) UND erneut über `check_call`,
das den Empfänger als impliziten self-Parameter durchreicht und dort wieder
`infer_type` darauf laufen lässt. T(n) = 2·T(n−1) = O(2ⁿ) über die Kettentiefe.

**Fix — Memoization.** `infer_type` cached das Ergebnis pro Node-Id (der
`expr_types`-HashMap existierte bereits, wurde aber nur beschrieben, nie zum
Kurzschließen gelesen). Am Anfang: bei vorhandener Id + Cache-Treffer sofort
zurückgeben. Sicher, weil nur EIN source via `infer_type` läuft (preludes werden
nur deklariert), Node-Ids darin eindeutig sind und der geklonte Empfänger seine
Id behält → trifft den Cache. Der `check`-Befehl bekam zusätzlich
`assign_node_ids` VOR dem Typecheck (lief nur im build-Pfad) — ohne Ids ist die
Memoization inaktiv. Nebeneffekt: keine duplizierten Typfehler mehr.

**Verifiziert:** Kette 24: build 28 s → **971 ms**, check 23 s → **2 ms**; Kette
300: check 8 ms (linear/quasi-konstant). e2e-Regressionstest
`tests/e2e/method_chain_linear.tnx` (60-fache Kette, korrektes Ergebnis 42);
`make check` voll grün (Memoization ändert kein Typ-Ergebnis). Codegen war NICHT
betroffen (build wurde mit dem typecheck-Fix allein linear).

---

## Bug 51 — Lexer: Zahlen-Literal über Wertebereich wurde still zu 0

**Status: GEFIXT (2026-07-20).** Beim Lexer-Fuzzing gefunden. Ein Ganzzahl-
Literal über i64-max (`99999999999999999999999999999999`, `0xF…F`, `0b1…1`)
wurde still zu **0** — silent garbage, dieselbe Klasse wie Bug 36/37/45. Ursache:
fünf `.parse().unwrap_or(0)` / `from_str_radix(…).unwrap_or(0)` im Lexer
(dezimal, hex, oktal, binär, float) schluckten Overflow/Format-Fehler.

**Fix.** `unwrap_or(0)` → `map_err(…)?` mit klarem Lexer-Fehler („integer literal
out of range for Int64: …", analog hex/oktal/binär; Float: „out of range for
Float64" via `is_finite()`-Prüfung, plus „invalid float literal" für
Format-Fehler). Die Lex-Funktionen geben bereits `Result<Token, Error>` zurück.

**Dabei aufgedeckte Zweit-Regression (im selben Fix gelöst):** der Zahlen-`text`
wurde NACH dem Suffix-Lesen erfasst (`read_float_suffix`/`read_int_suffix` bewegen
`pos` schon vorbei) → `text` enthielt das Suffix (`"3.14f32"`, `"42i32"`), das
`parse()` nun ablehnte. Der alte `unwrap_or(0.0)` hatte das maskiert (die Tests
prüften nur den Token-TYP, nicht den Wert). Fix: `num_end = self.pos` VOR dem
Suffix-Lesen, `text` aus `chars[start..num_end]`.

**Verifiziert:** Overflow-Literale (dezimal/hex/binär) → sauberer Lexer-Fehler;
gültige Grenz-/Basis-Literale (i64-max, hex, binär, oktal, Underscores, Float,
f32/f64-Suffix, i64::MIN+1) → korrekt gelext + gerechnet; alle 246 Lexer-Unit-
Tests grün; e2e-Regressionstest `tests/e2e/int_literal_overflow.tnx`; `make check`
voll grün.

**Bekannte Grenze:** exakt `i64::MIN` (`-9223372036854775808`) — das Literal
`9223372036854775808` passt allein nicht in i64 (nur nach dem Unary-Minus) → wird
abgelehnt. War vorher auch schon kaputt (→ 0); ein sauberer Fix bräuchte
negativ-Literal-Sonderbehandlung im Parser (v2). `i64::MIN+1` und alles andere ok.

---

## B1 — Getypte Struct-Layouts (Phase 1 von 5)

**Status: PHASE 1 GEFIXT (2026-07-20).** Erster Schritt, die uniforme i64-Wert-
darstellung (die Wurzel diverser Bug-Klassen, s. Sprach-Bewertung) durch echte
LLVM-Struct-Typen zu ersetzen. Architektur-Investition (IR-Qualität, opt-
Verifikation, Fundament für B2), kein akuter Korrektheitsgewinn — der ist durch
Bug 37 (undeklarierte Felder → Typfehler) bereits abgedeckt.

**Kernidee — Layout-Identität.** Jedes Feld ist physisch ein 8-Byte-Slot (die
Store-Seite schreibt immer i64-Bits). Ein named type `%class.Foo = type { double,
i64, i8* }` mit zum 8-Byte-Slot normalisierten Feldtypen ist damit BYTE-IDENTISCH
zum bisherigen `[N x i64]`-Layout — ein getyptes `getelementptr %class.Foo, …,
i32 <idx>` und das alte `getelementptr i64, …, i64 <idx>` treffen dieselbe
Adresse. Deshalb sind getypte und i64-Zugriffe während der Migration MISCHBAR, und
die C-Runtime (die Objekte über i64-Offsets anfasst) bleibt unberührt.

**Phase 1 (dieser Commit):** `emit_struct_type_defs` gibt `%class.<name>`-Typen für
PLAIN Klassen aus (nicht-generisch, nicht-spezialisiert, kein Float32-Feld —
Letzteres hat einen latenten i64→float-bitcast-Bug im Alt-Pfad). Der FieldAccess-
READ nutzt für diese Klassen ein getyptes GEP + direktes `load <slot>` statt
i64-Load+bitcast (`slot_llvm_ty`: double/ptr bleiben, alles andere → i64). Ein
`load double`/`load i8*` an der i64-geschriebenen Adresse ist ein valider Type-Pun
gleicher Größe → gleicher Wert wie Alt-Pfad, aber opt kann den Offset verifizieren.
Alle anderen Klassen (generisch, spezialisiert, mit Float32) fallen auf den
i64-Pfad zurück (identisches Layout).

**Verifiziert:** Float-/String-/Int-/Objekt-Referenz-Felder + verschachtelte
Objektzugriffe korrekt; named types im IR, getypte GEP im Einsatz; e2e-
Regressionstest `tests/e2e/typed_struct_layout.tnx`; `make check` voll grün (die
Layout-Identität hält über die gesamte Stdlib inkl. C-Runtime).

**Phase 2 (2026-07-20):** Write-Pfad im StructLiteral-Konstruktor getypt. Die
Feld-Stores nutzen jetzt getyptes GEP + `store <slot>` (via neuer `coerce_to_slot`:
Wert → 8-Byte-Slot double/ptr/i64) statt i64-slot + ptrtoint/bitcast. Bool (i1)
→ i64-Slot via zext, wie zuvor. Für Klassen ohne named type unverändert i64-Pfad.
Der Alloc (`tinox_alloc`) und der Vtable-Slot-Store bleiben i64 (Phase 3).
Verifiziert: double-/String-/Int-/Bool-/Objekt-Referenz-Felder korrekt geschrieben
+ gelesen; e2e-Test um die Store-Seite erweitert; `make check` grün.

**Phase 3 (2026-07-20):** Feld-Assignments (`obj.f = v`) getypt. Beide Haupt-
Assignment-Pfade (StmtKind::Assignment + ExprKind::Assign, FieldAccess-Target)
nutzen jetzt einen gemeinsamen Helper `try_typed_field_store` (getyptes GEP +
`store <slot>`, sonst false → unveränderter i64-Fallback jeder Stelle). **Vererbung
war bereits kostenlos abgedeckt** (der named type enthält Parent-Felder via
`collect_inherited_fields`; Read + Assignment auf geerbten Feldern verifiziert).
Der `New`-Konstruktor (positional args, selten) und Tuple-Stores bleiben i64.
Verifiziert: getypte Assignments auf double-/String-/Bool-/geerbten Int-/Objekt-
Feldern; `make check` grün.

**Phase 4 (2026-07-20):** generische Spezialisierungen (`Box__i64`) bekommen jetzt
auch named struct types + getypte Feldzugriffe. Herausforderung war das Timing:
Spezialisierungen entstehen on-demand MITTEN in der Emission (im Second Pass),
aber ein forward-referenzierter named type ist opaque/unsized → `getelementptr`
wird vom Verifier abgelehnt (`base element must be sized`). Lösung: die
Spezialisierungs-type-defs werden in `spec_type_defs` gesammelt und in `into_ir`
an einem `@@SPEC_TYPES@@`-Marker (VOR allen Funktionsrümpfen) eingesetzt. Helper
`register_named_struct_type` von Phase 1 wiederverwendet. Verifiziert: getypter
Read + Assignment auf `Box<Int64>`-Feldern korrekt; named types `%class.Box__i64`
im IR; `make check` grün (generic-lastige Stdlib — Cache/Option/Result/collections
— unverändert).

**Dabei bestätigte vorbestehende Bugs (NICHT Phase 4, git-stash-geprüft):** ein
generisches `T`-Feld wird im TYPECHECK nicht zur Instanz aufgelöst —
`Box<String>::get()` gibt den Zeiger als Zahl aus, `pair.tField.toString()` →
„undefined function: T_toString", `pair.tField = literal` → „expected T, found
String". Betrifft die Typecheck-seitige Typ-Auflösung generischer Felder, nicht den
(jetzt getypten) Codegen-Feldzugriff. Eigener Bug-Komplex.

**Phase 5 (2026-07-20) — ABSCHLUSS.** Der `unwrap_or(0)`-Offset-Fallback an den
getypten Feldzugriffs-Stellen (Read + beide Assignment-Pfade via
`try_typed_field_store`) ist jetzt ein harter Codegen-Fehler statt eines stillen
Zugriffs auf Offset 0 — die letzte silent-garbage-Quelle im Feld-Codegen (Helper
`checked_typed_offset`). **Empirisches Ergebnis (wie in der Sondierung vermutet):
der Fallback wird NIE erreicht** — `make check` bleibt grün, weil Bug 37 (Typecheck
lehnt unbekannte Felder ab) den Fall schon vorher abfängt. Die Härtung ist also
defense-in-depth (feuert nur bei einer internen Layout-Inkonsistenz), kein neuer
Fund. Damit ist der Korrektheits-Payoff von B1 formal abgesichert.

**B1-Bilanz (5 Phasen):** Read (1), StructLiteral-Store (2), Feld-Assignments +
Vererbung (3), generische Spezialisierungen (4), Offset-Härtung (5). Plain- UND
generische Klassen nutzen jetzt echte LLVM-Struct-Typen (`%class.<name>`) für
Feldzugriff statt uniformer i64-Slots + bitcast — dank Layout-Identität (jedes Feld
8-Byte-Slot) risikoarm inkrementell, jede Phase mit grünem `make check`.
**Weiterhin auf i64-Pfad (bewusst):** Float32-Feld-Klassen (latenter Cast-Bug),
`New`-Konstruktor (positional args, selten), Vtable-Slot-Store, Tuple-Stores — alle
mischbar via Type-Pun. **Fundament für B2** (getypte Werte durch die ganze Codegen)
steht. **Nicht gelöst (Wurzel bleibt):** die uniforme i64-Wertdarstellung von
*Werten* (nicht Feldern) — Methoden-Rückgaben, Locals, Args — inkl. der dabei
gefundenen generischen T-Feld-Typecheck-Bugs (s. Phase 4).

---

## Bug 52 — generische Instanzmethode ohne T-Param wählte falsche Spezialisierung (erster B2-Schritt)

**Status: GEFIXT (2026-07-20).** Der erste der in B1-Phase 4 gefundenen
generischen Wert-Bugs — und der Einstieg in B2 (getypte Werte). `Box::get(bs)` mit
`bs: Box<String>` rief fälschlich `@Box__i64_get` (Int64-Spezialisierung) statt
`@Box__i8P_get` und gab den String-Zeiger als Zahl aus (silent garbage, `4263940`).

**Ursache (codegen, generische Bindungsinferenz).** Für den Spezialisierungs-Aufruf
werden die Typbindungen aus den Argumenten abgeleitet. Bei einer Methode OHNE
T-Parameter (`fn get() -> T` nutzt T nur im Rückgabetyp) läuft die Param-Schleife
leer → T fällt auf den **i64-Default** → `Box__i64` gewählt. Der implizite
Empfänger `bs` (args[0] bei arg_offset==1, Bug 38) wurde nicht als Bindungsquelle
genutzt.

**Fix.** Vor dem i64-Default: ist es ein this-Stil-Aufruf (arg_offset==1), wird der
Marker des Empfängers (args[0], z.B. `Box__i8P`) in die Bindungen zerlegt
(`Box__` strippen → `i8P` → T=`i8*`) — dieselbe Zerlegung wie beim Cache::set-Stil,
aber auf den impliziten Empfänger angewandt. Für Methoden ohne T-Param ist das die
einzige Bindungsquelle. B1-Phase 4 hatte die Zielfunktion `@Box__i8P_get` bereits
korrekt getypt (`ret i8*`); es fehlte nur die richtige AUSWAHL an der Call-Site.

**Verifiziert:** `Box<String>/Int64/Float64::get(obj)` → hi/42/3.5 (korrekte
Spezialisierung + Rückgabetyp); e2e-Test `tests/e2e/generic_receiver_binding.tnx`;
`make check` grün (generic-lastige Stdlib Cache/Option/Result unverändert).

**Noch offen (die anderen zwei Phase-4-Bugs, TYPECHECK-seitig, größerer Refactor):**
der Typechecker trägt für Instanzen nur `ValueType::Named("Box")` OHNE Typargumente
— `Box<Int64>` und `Box<String>` sind ununterscheidbar, ein `T`-Feld bleibt
unaufgelöst. Daher `bi.value.toString()` → „undefined function: T_toString" und
`box.tField = literal` → „expected T, found String". Der Fix erfordert, dass
`ValueType` generische Typargumente führt und sie durch Feldzugriffe/Methoden
propagiert (die Typecheck-Seite von B2) — großer Refactor, eigener Komplex.

---

## Bug 53 — B2 Schritt 1: ValueType trägt generische Typargumente (T-Feld-Auflösung)

**Status: GEFIXT (2026-07-20).** Der in Bug 52 als „großer Refactor" umrissene
Typecheck-seitige B2-Komplex — überraschend sauber gelandet. Behebt die zwei
verbleibenden generischen T-Feld-Bugs aus B1-Phase 4: `bi.value.toString()`
(bi: Box<Int64>) → „undefined function: T_toString" und `box.tField = literal` →
„expected T, found String".

**Ursache.** `ValueType::Named(String)` trug NUR den Klassennamen — die
Typargumente wurden in `from_parser_type` (`Type::Generic { name, .. } →
Named(name)`) verworfen. `Box<Int64>` und `Box<String>` waren ununterscheidbar;
ein Feld `value: T` blieb der unaufgelöste Typparameter `Named("T")`, auf dem
`.toString()`/Assignment scheiterten.

**Fix (3 Teile).** (1) `Named(String)` → `Named(String, Vec<ValueType>)`;
`from_parser_type` füllt die Args (`Box<Int64>` → `Named("Box", [Int])`). (2) Neues
Register `class_type_params` (`Box` → `["T"]`) + Helper `substitute_type_params`:
löst einen Feld-/Rückgabetyp gegen die Instanz-Args auf (`Named("T")` bei Box mit
`[Int]` → `Int`; rekursiv in Array/Map/Nullable). Angewandt in der FieldAccess-
Typinferenz. (3) **Custom `PartialEq`**: zwei `Named` sind gleich gdw ihre Namen
gleich sind — die Args sind Zusatzinfo für die Substitution, NICHT Teil der
Typidentität. Hält jeden bestehenden `==`-Vergleich exakt wie vorher (`Box<Int>`
und `Box<String>` galten schon immer als gleich) und verhindert Fehlalarme wie
„expected Box, found Box" (Rückgabetyp `Box<T>` vs StructLiteral `Named("Box",[])`).

**Refactor-Sicherheit:** die ~46 `Named`-Stellen wurden durch Rusts Typprüfung
erzwungen (ein vergessenes `Named` = Compile-Fehler, kein stiller Bug) — deshalb
war dieser zentrale Typ-Refactor risikoarm trotz Breite.

**Verifiziert:** `Box<Int64/String/Float64>`-T-Feld Read + `.toString()` +
Assignment korrekt; Nicht-T-Felder unverändert; e2e-Test
`tests/e2e/generic_field_type_resolution.tnx`; `make check` voll grün (der zentrale
ValueType/PartialEq-Umbau bricht nichts).

**B2 Schritt 2 SONDIERT (2026-07-20) — nicht sauber machbar, zurückgestellt.**
Typargument-INFERENZ für nicht-annotierte Bindungen (`let bi = Box::make(42)` ohne
`: Box<Int64>`). Die Typecheck-Hälfte wurde implementiert und funktionierte
isoliert (Register `generic_method_param_types` mit unerased Param-Typen + Helfer
`unify_param`/`substitute_bindings`: leitet aus `make(42)` T=Int ab → Rückgabetyp
`Named("Box", [Int])`). ABER: sie allein verwandelt einen sauberen Typecheck-Fehler
in einen **Codegen-ICE** (`use of undefined value @T_toString`) — deshalb wieder
zurückgenommen. Grund: **der Codegen hat ein EIGENES Typ-System (Marker-Sprache),
getrennt vom Typecheck-`ValueType`.** `to_marker(Named("Box",[Int]))` liefert nur
`"Box"` (verwirft die Args), und `infer_struct_type` für den let-Wert nutzt
`method_ret_class["Box_make"] = "Box"` (erased Basis), nicht die im EnumValue-
Codegen abgeleitete Spezialisierung `Box__i64`. Der Codegen ruft `@Box__i64_make`
zwar korrekt (Bug 52), aber `bi`s MARKER bleibt `"Box"` → `bi.value` löst zu `T`
auf → `@T_toString`. Ein vollständiger Schritt 2 braucht also BEIDE Seiten:
Typecheck-Inferenz UND Codegen-Marker-Propagierung (die Spezialisierung durch
`infer_struct_type`/`method_ret_class` bis zum let-Marker tragen) — eine Verbindung
der zwei getrennten Typ-Systeme, der eigentliche tiefe B2-Kern. Kein sauberer
Ein-Zug-Schritt; eigener Komplex. **Der annotierte Fall (Schritt 1) deckt den
häufigen Weg ab** (wie in Java, wo generische Konstruktoren i.d.R. annotiert werden).

---

## Bug 54 — `charCodeAt` out-of-bounds las über das String-Ende → Müll

**Status: GEFIXT (2026-07-20).** Beim String/Unicode-Fuzzing gefunden.
`"ABC".charCodeAt(100)` gab `140226739651968` (ein Zeiger-Wert als Zahl) statt
eines definierten Werts — silent garbage, dieselbe Klasse wie der Rest der Session.

**Ursache (codegen).** `charCodeAt` emittierte einen INLINE `getelementptr i8 +
load i8` OHNE Bounds-Check. Bei einem Index jenseits der String-Länge (oder
negativ) las es beliebigen Speicher → UB/Müll.

**Fix.** Neue bounds-geprüfte Runtime-Funktion `tinox_string_char_code_at(s, idx)`
(gibt -1 für idx<0 oder idx>=len, sonst das Byte); der Codegen emittiert jetzt
einen Call statt des ungeprüften Inline-Loads. -1 ist konsistent mit `indexOf`s
„nicht gefunden".

**Verifiziert:** gültige Indizes unverändert (65/67); out-of-bounds/negativ/leerer
String → -1; e2e-Test `tests/e2e/char_code_at_bounds.tnx`; `make check` grün (die
Stdlib-Nutzer hex/uri/encoding/hash iterieren mit `i < len()` → unberührt).

**Sondierungs-Fazit String/Unicode (sonst kein Bug):** die übrigen String-Ops
sind byte-basiert (Go-Modell) — `.len()` gibt Bytes, `substring` nimmt Byte-
Offsets. Das ist eine DURCHGÄNGIGE, KONSISTENTE Design-Entscheidung (92 Stdlib-
`substring`-Aufrufe nutzen `indexOf`+`substring`+`len` byte-konsistent, korrekt für
ASCII-Delimiter). `substring` über eine Multibyte-Zeichengrenze erzeugt korruptes
UTF-8 (`"café".substring(0,4)` → `"caf�"`) — der bekannte Kompromiss byte-basierter
Sprachen, KEIN Fix-fähiger Bug (eine Umstellung auf zeichen-basiert bräche alle 92
byte-konsistenten Stdlib-Stellen). `toUpperCase`/`toLowerCase` sind ASCII-only.
indexOf/replace/trim/split/startsWith/endsWith + out-of-bounds-substring sind
robust und korrekt (kein Crash/Hänger).

---

## Bugs 55–57 — Array-OOB + Division-durch-Null (silent garbage) + der dadurch aufgedeckte Short-Circuit-Bug

**Status: ALLE GEFIXT (2026-07-21).** Bei der Runtime-Ops-Bounds-Jagd (Fortsetzung
von Bug 54) gefunden — und ein Fix deckte einen fundamentalen dritten Bug auf
(Bug-37-Methodik).

**Bug 55 — Array-Index out-of-bounds → Müll.** `xs[100]` auf einem 3-Element-Array
gab einen Zeiger-Wert als Zahl (unchecked inline `getelementptr + load`). Fix:
bounds-geprüfte Runtime-Funktion `tinox_array_get(handle, idx)` → harter Fehler
„array index out of bounds: N (length M)" statt Müll (wie Javas
ArrayIndexOutOfBoundsException).

**Bug 56 — Integer-Division/Modulo durch Null → Müll.** `10 / 0` gab Müll (LLVM-UB;
`opt` faltete `sdiv i64 x, 0` zu einem beliebigen Wert). Fix: `tinox_checked_sdiv`/
`tinox_checked_srem` (i64-Pfad) → harter Fehler „division/modulo by zero"; auch der
`INT64_MIN / -1`-Overflow-UB ist jetzt definiert (wrap, wie in Java).

**Bug 57 — `&&` / `||` kurzschlossen NICHT (fundamental).** Aufgedeckt, weil der
neue Array-Bounds-Check (55) den Heap-Smoke-Test crashte: `siftDown`s Guard
`left < len && items[left] < …` las `items[left]` trotz `left >= len`. Ursache: der
Codegen emittierte `and i1`/`or i1` auf zwei EAGER (vorab) ausgewerteten Operanden
→ die RHS lief IMMER. Das brach jeden Guard (`i < len && arr[i]`, `ptr != null &&
ptr.f`, `d != 0 && x/d`) und jeden gewollten Seiteneffekt-Kurzschluss. Vorher
maskiert, weil der OOB-Read still Müll las, den `false &&` verwarf. Fix: `&&`/`||`
werden VOR der Operanden-Auswertung abgefangen und als Branch emittiert — LHS
auswerten, dann die RHS nur in ihrem eigenen Block (Ergebnis über einen i1-Slot).

**Verifiziert:** RHS läuft nicht bei `false &&`/`true ||`; Guards schützen vor
OOB/div0; kaskadierte + gemischte Logik korrekt; gültige Array-/Div-Pfade
unverändert. e2e-Test `tests/e2e/short_circuit_eval.tnx`; zwei Codegen-Unit-Tests
auf die neue Branch-Emission umgestellt; `make check` voll grün (Heap-Modul jetzt
korrekt — der Guard schützt wirklich). **Der Short-Circuit-Bug war der wichtigste
Fund der Session** — fundamental und die ganze Zeit latent, sichtbar erst durch die
Bounds-Härtung.

**Bewusste Grenze:** die checked-div-Umstellung greift nur für den i64-Pfad
(uniform-ABI-Hauptfall); kleinere Int-Typen (i32 etc., selten) behalten das rohe
`sdiv`. Array-Index-OOB und div0 sind harte Aborts (nicht fangbar) — Java würfe
fangbare Exceptions; ein `throw` wäre v2, der Abort ist der sichtbare 80/20-Fix.

---

## Bugs 58–59 — String-Index + first()/last() ungeprüft (Bounds-Härtung, Forts.)

**Status: GEFIXT (2026-07-21).** Weiter dem Bug-54-Faden gefolgt: übrige
Runtime-Ops mit unchecked Reads.

**Bug 58 — String-Index `s[i]`.** Machte denselben unchecked inline `getelementptr
i8 + load` wie `charCodeAt` vor Bug 54 (`"hi"[100]` las hinter den String, hier
zufällig 0). Fix: über `tinox_string_char_code_at` (bounds-geprüft, -1 out-of-range).

**Bug 59 — `first()` / `last()` auf leerem Array.** `first()` las Element 0
ungeprüft; `last()` rechnete `len-1` = **-1** bei leerem Array → Read VOR dem
Buffer. Beide über `tinox_array_get` geleitet → harter Fehler bei leerem Array. Der
echte Instance-Pfad (MethodCall, mit `is_str`-inttoptr) war ein ANDERER als der
zuerst gefixte (5068, static) — beide umgestellt.

**Verifiziert:** String-Index gültig (104) + OOB → -1; `first`/`last` auf Int- und
String-Arrays korrekt (10,30 / a,c); leeres `first()` → „array index out of bounds:
0 (length 0)"; e2e-Test `tests/e2e/string_index_first_last_bounds.tnx`; `make check`
grün (keine latenten Stdlib-Bugs diesmal — first/last nie auf leeren Arrays genutzt).

---

## Verwandte Codegen-Fixes (bereits implementiert, als Referenz)

Diese Fixes wurden in `crates/tinox-codegen/src/codegen.rs` vorgenommen, um die Tests
zum Laufen zu bringen (ohne die obigen Bugs grundlegend zu fixen):

1. **`method_ret_class` für `Type::Map`-Rückgaben** (Zeile ~822):
   ```rust
   } else if matches!(&method.ret_type, Type::Map(_, _)) {
       self.method_ret_class.insert(key.clone(), "Map".to_string());
   }
   ```
   Damit erkennt `let o = SomeClass::asObject(v)` die Typ-Info und Map-Dispatch funktioniert.

2. **`infer_struct_type` für `EnumValue` und `MethodCall`** (Zeile ~463):
   ```rust
   ExprKind::EnumValue { enum_name, variant, .. } => {
       let key = format!("{}_{}", enum_name, variant);
       self.method_ret_class.get(&key).cloned()
   }
   ExprKind::MethodCall { obj: mc_obj, method: mc_method, .. } => {
       self.infer_struct_type(mc_obj, ctx)
           .and_then(|obj_class| {
               let key = format!("{}_{}", obj_class, mc_method);
               self.method_ret_class.get(&key).cloned()
           })
   }
   ```
   Damit funktioniert `asObject(v).len()` in Kettenaufrufen korrekt.
