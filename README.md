# Tinox Programming Language

**Tinox** ist eine native, statisch typisierte Programmiersprache mit LLVM-Backend, Garbage Collection und Concurrency-Support.

> Benannt nach **Tino** + **Linux**/Unix – eine Sprache für moderne, lesbare Software.

## Status

**Phase:** V2 – Feature-Complete Core (In Development)

| Komponente   | Status      | Notizen                              |
|--------------|-------------|--------------------------------------|
| Lexer        | ✅ Fertig   | Unicode, String-Interpolation, Ranges |
| Parser       | ✅ Fertig   | Vollständiger AST                    |
| Type Checker | ✅ Fertig   | Basistypen, Klassen, Enums, Generics |
| Code Gen     | ✅ Fertig   | LLVM IR Backend                      |
| Runtime      | ✅ Fertig   | C Runtime (pthread-basiert)          |
| CLI          | ✅ Fertig   | build, run, check                    |

## Installation

```bash
git clone https://github.com/subnix-work/tinox.git
cd tinox
cargo build --release
# Binary: target/release/tinox
```

Voraussetzungen: `clang`, `llc` (LLVM-Tools)

## Usage

```bash
tinox run program.tinox           # Kompilieren und ausführen
tinox build program.tinox         # Kompilieren (erzeugt ./a.out)
tinox check program.tinox         # Nur Type-Check
tinox fmt program.tinox           # Formatieren (Ausgabe auf stdout)
tinox fmt --write program.tinox   # Formatieren und Datei überschreiben
```

## Hello World

```tinox
fn main() -> Int64
{
    println("Hello, World!");
    return 0;
}
```

## Syntax-Überblick

### Variablen

```tinox
let x: Int64 = 42;          // immutable
var y: Float64 = 3.14;      // mutable
let name = "Tino";          // Typ-Inferenz
let msg = "Hi ${name}!";    // String-Interpolation
```

### Namespaces & statische Methoden

Alle Funktionen leben in Klassen. Statische Methoden (kein Objekt nötig) verwenden `fnc`:

```tinox
namespace math {
    class Utils {
        fnc add(a: Int64, b: Int64) -> Int64
        {
            return a + b;
        }

        fnc square(x: Int64) -> Int64
        {
            return x * x;
        }
    }
}

fn main() -> Int64
{
    println(Utils.add(3, 4));    // 7
    println(Utils.square(5));    // 25
    return 0;
}
```

Import des ganzen Namespaces oder einer einzelnen Klasse:

```tinox
import math;          // alle Klassen aus math
import math.Utils;    // nur Utils
```

### Klassen & Vererbung

Instanzmethoden verwenden `fn` und haben Zugriff auf `this`:

```tinox
class Animal
{
    name: String;

    fn speak() -> String
    {
        return "...";
    }
}

class Dog extends Animal
{
    fn speak() -> String
    {
        return "Woof! I am ${this.name}";
    }
}
```

### Interfaces

```tinox
interface Printable
{
    fn toString() -> String;
}

class Point implements Printable
{
    x: Int64;
    y: Int64;

    fn toString() -> String
    {
        return "(${this.x}, ${this.y})";
    }
}
```

### Enums & Pattern Matching

```tinox
enum Direction
{
    North;
    South;
    East;
    West;
}

namespace nav {
    class DirUtils {
        fnc turn(d: Direction) -> Direction
        {
            match d
            {
                North => return East;
                East  => return South;
                South => return West;
                West  => return North;
            }
        }
    }
}
```

### Generics

```tinox
fn identity<T>(x: T) -> T
{
    return x;
}

class Box<T>
{
    value: T;

    fn get() -> T
    {
        return this.value;
    }
}

let b = new Box<Int64>(42);
println(b.get());
```

### Tuples

```tinox
let point = (10, 20);
println(point.0 + point.1);

let nested = ((1, 2), 3);
println(nested.0.1);        // 2
```

### Arrays & Builtins

```tinox
let arr = [1, 2, 3, 4, 5];
arr.push(6);
println(arr.len());         // 6
println(arr.first());       // 1
println(arr.last());        // 6

let s = "hello";
println(s.toUpper());       // HELLO
println(s.contains("ell")); // true

let parts = "a,b,c".split(",");
println(parts.len());       // 3
println(parts.join(" - ")); // a - b - c
```

### Maps

```tinox
let m = @{"one" => 1, "two" => 2};
m.insert("three", 3);
println(m.get("one"));      // 1
println(m.contains("two")); // true
println(m.len());           // 3
m.remove("one");
```

### Ranges & Schleifen

```tinox
for i in 0..5               // 0, 1, 2, 3, 4 (exklusiv)
{
    println(i);
}

for i in 0...5              // 0, 1, 2, 3, 4, 5 (inklusiv)
{
    println(i);
}

for ch in "hello"           // Zeichen-Iteration
{
    print(ch);
}
```

### Async / Concurrency

```tinox
async fn fetchData(id: Int64) -> Int64
{
    return id * 2;
}

fn main() -> Int64
{
    let handle = spawn fetchData(21);
    let result = await handle;      // 42
    println(result);

    let ch = channel;
    send ch -> 99;
    let val = recv ch;
    println(val);
    return 0;
}
```

### File I/O

```tinox
// Schreiben
let f = open("output.txt", "w");
f.write("Hallo Tinox!\n");
f.close();

// Lesen (ganzer Inhalt)
let f = open("output.txt");
let content = f.read();
f.close();
println(content);

// Zeilenweise lesen
let f = open("log.txt");
while !f.eof()
{
    let line = f.readLine();
    println(line);
}
f.close();

// Hilfsfunktionen
println(fileExists("output.txt")); // true
deleteFile("output.txt");
```

Modi: `"r"` (lesen, default), `"w"` (schreiben), `"a"` (anhängen), `"rb"` / `"wb"` (binär)

### Defer

```tinox
fn readFile(path: String) -> String
{
    let f = open(path);
    defer { f.close(); }   // wird automatisch am Funktionsende ausgeführt

    return f.read();
}
```

Mehrere `defer`s werden in umgekehrter Reihenfolge ausgeführt (LIFO):

```tinox
defer { println("3"); }
defer { println("2"); }
defer { println("1"); }
// Ausgabe beim Return: 1, 2, 3
```

### Try / Catch

```tinox
try
{
    riskyOperation();
}
catch e: RuntimeError
{
    println("Fehler aufgetreten");
}
finally
{
    cleanup();
}
```

## Feature-Übersicht

| Feature                      | Status     |
|------------------------------|------------|
| Variablen (let/var)          | ✅ Fertig  |
| Namespaces                   | ✅ Fertig  |
| Klassen + Vererbung          | ✅ Fertig  |
| Statische Methoden (`fnc`)   | ✅ Fertig  |
| Interfaces + vtable          | ✅ Fertig  |
| Enums + Pattern Matching     | ✅ Fertig  |
| Generics (Monomorphisierung) | ✅ Fertig  |
| Tuples                       | ✅ Fertig  |
| Arrays + Builtins            | ✅ Fertig  |
| String-Interpolation         | ✅ Fertig  |
| Ranges (.. / ...)            | ✅ Fertig  |
| Lambdas / Closures           | ✅ Fertig  |
| Async / Spawn / Await        | ✅ Fertig  |
| Channels + Select            | ✅ Fertig  |
| Try / Catch / Finally        | ✅ Fertig  |
| Import-System                | ✅ Fertig  |
| Float32 / Float64            | ✅ Fertig  |
| Map / Dict-Typ               | ✅ Fertig  |
| `defer`-Statement            | ✅ Fertig  |
| LSP (tinox-lsp)              | ✅ Fertig  |
| Eclipse Plugin               | ✅ Fertig  |
| File I/O                     | ✅ Fertig  |
| Formatter (`tinox fmt`)      | ✅ Fertig  |
| REPL                         | ⏳ Geplant |

## Projekt-Struktur

```
tinox/
├── Cargo.toml
├── crates/
│   ├── tinox-common/       # Shared types (Span, Error)
│   ├── tinox-lexer/        # Lexer / Tokenizer
│   ├── tinox-parser/       # Parser + AST
│   ├── tinox-typecheck/    # Type Checker
│   ├── tinox-codegen/      # LLVM IR Code Generation
│   ├── tinox-lsp/          # Language Server Protocol
│   └── tinox/              # CLI Binary
└── runtime/                # C Runtime (tinox_alloc, threading, channels)
```

## Lizenz

MIT OR Apache-2.0
