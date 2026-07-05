# Tinox Compiler Bugs

Discovered while porting jq to Tinox (`/home/tg7c49/git/jgrep-tinox`).
Each bug has a minimal reproduction and a description of expected vs. actual behavior.
Fix them in order — later bugs may depend on earlier fixes being in place.

---

## Bug 1 — `s.len()` auf match-gebundenen Strings gibt ASCII-Code zurück

**Status: GEFIXT (2026-07-05)** — Match-Bindings verwenden jetzt den echten LLVM-Typ aus der Enum-Deklaration (`bind_match_payload` + `enum_variant_payloads`-Pre-Pass in codegen.rs). String-Payloads werden als `i8*` gebunden, damit greift der normale String-Dispatch.

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

## Bug 8 — (nicht vergeben / offen für neue Findings)

---

## Bug 9 — Verschachtelte Filter-Enum-Payloads werden korrupt

**Status: GEFIXT (2026-07-05)** — Nicht mehr reproduzierbar nach dem Match-Binding-Fix (Bug 1/2/5/13/14). Die Korruption lag im Auslesen der Payloads per Match, nicht im Speichern. Verifiziert mit rekursivem `Pipe(Filter, Filter)`-Test.

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
