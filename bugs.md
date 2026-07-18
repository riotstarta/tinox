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

**Status: TEILWEISE GEFIXT** — Befund des neuen Stdlib-Smoke-Gates
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
und pool sind seit Bug 27 gefixt. Stand nach Bug 27: 56/61 grün.
Verbleibende KNOWN_BROKEN (5): Klasse 4 (Lambda/Handler)
events/logger/rest_framework, Klasse 5 (Frontend) http2_server/ini.

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
