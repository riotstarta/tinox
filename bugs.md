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

**Status: OFFEN (2026-07-17)** — Befund des neuen Stdlib-Smoke-Gates
(`crates/tinox/tests/stdlib_smoke.rs`, Teil von `make e2e`). Kein Test hatte
diese Module je importiert; da ein Import das ganze Modul codegen't, reichte
ein minimaler Aufruf pro Modul, um alles Folgende aufzudecken. Die exakte
Liste steht in `KNOWN_BROKEN` im Test — jedes gefixte Modul MUSS dort
ausgetragen werden (sonst „stale entry"), jedes neue Modul braucht einen
Smoke-Fall (Vollständigkeits-Test).

**Grün verifiziert (19):** array, bitmap, csv, encoding, env, format, hash,
hpack, http_server, json, math, mathx, pool, semaphore, sort, tpl, trie,
validation, yaml.

**Fehlerklassen (42 kaputt):**

1. **Ghost-Builtins** — Modul ruft Funktionen, die weder in `runtime/runtime.c`
   existieren noch im Codegen deklariert sind → ICE „use of undefined value":
   - http: `httpGet/httpPost/httpPut/httpDelete/httpPatch/httpSetHeader/httpClearHeaders/httpStatusCode/httpBody/httpHeader`
   - socket: `socketConnect/socketBind/socketListen/socketSend/socketReceive/socketClose`
   - base64: `base64Encode/base64Decode` · crypto: `md5Hash/sha256Hash/hmacSha256Hash`
   - uuid: `uuidGenerate` · uri: `uriEncode/uriDecode/uriEncodeComponent/uriDecodeComponent`
   - xml: `xmlTagName/xmlAttr/xmlChildren/xmlTextContent` · zip: `zipListEntries/zipExtractFile/zipAddFile/zipRemoveFile`
   - regex: `regexFindFirst/regexReplaceAll` (isMatch/findAll/split existieren)
   - fs: `fileDelete` · process: `processId` · time: `now` · debug: `gcCollect`
   - random: `randomInt/randomFloat` · mathf: `cosf/sinf/tanf/logf/log10f` (float-libm nie deklariert)
   - string: `String_reverse` · io: `String_lastIndexOf` (String-Methoden ohne Codegen-Dispatch)
   - metrics: `__tinox_counter_inc/__tinox_histogram_record/…` (Typecheck-Fehler; Runtime hat `tinox_counter_inc` ohne Unterstriche)
   - jwt, rest: transitiv kaputt (crypto- bzw. http-Ghosts)
2. **Generics** — Instanzmethoden generischer Klassen werden nicht
   emittiert/gebunden: `Option_unwrap`/`Result_unwrap` undefined;
   `Stack<Int64>`-Annotation bindet T nicht (`expected T, found Int64`).
   Betroffen: option, result, cache, collections.
3. **Ungültige Casts** — `ptr → i64/double` (z. B. `sitofp i8* %value to double`)
   in Modul-Klassen mit String/Float-Feldern: complex, cron, decimal, fmt, toml.
4. **Lambda-/Handler-Codegen** — `%handler` i64 vs. ptr (events),
   Typ-Mismatch (logger), „unable to create block named 'entry'" (rest_framework).
5. **Frontend** — http2_server.tnx parst nicht (Zeile 770, „expected
   Semicolon, found Equals"); ini.tnx Typecheck-Fehler (`Map == null`).
6. **Laufzeit-Fehlverhalten** — kompiliert, rechnet falsch: hex (falsche
   Ausgabe), heap/set (Pointer statt Wert), iter (`repeat(7,3).len()` → 7),
   graph (961 statt 1), queue (leere Ausgabe), asm/ratelimit (Crash, Exit -1).

**Empfohlene Reihenfolge:** Klasse 2–4 sind Compiler-Bugs (fixen), Klasse 1
ist eine Produktentscheidung pro Modul (Runtime-Funktion implementieren oder
Modul streichen), Klasse 5–6 sind Modul-Bugs im .tnx-Code.

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
