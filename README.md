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
tinox run program.tinox      # Kompilieren und ausführen
tinox build program.tinox    # Kompilieren (erzeugt ./a.out)
tinox check program.tinox    # Nur Type-Check
```

## Hello World

```tinox
fn main() -> Int64 {
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

### Funktionen

```tinox
fn add(a: Int64, b: Int64) -> Int64 {
    return a + b;
}

fn swap(a: Int64, b: Int64) -> (Int64, Int64) {
    return (b, a);
}
```

### Klassen & Vererbung

```tinox
class Animal {
    name: String;

    fn speak() -> String {
        return "...";
    }
}

class Dog extends Animal {
    fn speak() -> String {
        return "Woof! I am ${this.name}";
    }
}
```

### Interfaces

```tinox
interface Printable {
    fn toString() -> String;
}

class Point implements Printable {
    x: Int64;
    y: Int64;

    fn toString() -> String {
        return "(${this.x}, ${this.y})";
    }
}
```

### Enums & Pattern Matching

```tinox
enum Direction {
    North;
    South;
    East;
    West;
}

fn turn(d: Direction) -> Direction {
    match d {
        North => return East;
        East  => return South;
        South => return West;
        West  => return North;
    }
}
```

### Generics

```tinox
fn identity<T>(x: T) -> T {
    return x;
}

class Box<T> {
    value: T;

    fn get() -> T {
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
```

### Ranges & Schleifen

```tinox
for i in 0..5 {             // 0, 1, 2, 3, 4 (exklusiv)
    println(i);
}

for i in 0...5 {            // 0, 1, 2, 3, 4, 5 (inklusiv)
    println(i);
}

for ch in "hello" {         // Zeichen-Iteration
    print(ch);
}
```

### Async / Concurrency

```tinox
async fn fetchData(id: Int64) -> Int64 {
    return id * 2;
}

fn main() -> Int64 {
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

### Try / Catch

```tinox
try {
    riskyOperation();
} catch e: RuntimeError {
    println("Fehler aufgetreten");
} finally {
    cleanup();
}
```

## Feature-Übersicht

| Feature                  | Status     |
|--------------------------|------------|
| Variablen (let/var)      | ✅ Fertig  |
| Funktionen               | ✅ Fertig  |
| Klassen + Vererbung      | ✅ Fertig  |
| Interfaces + vtable      | ✅ Fertig  |
| Enums + Pattern Matching | ✅ Fertig  |
| Generics (Monomorphisierung) | ✅ Fertig |
| Tuples                   | ✅ Fertig  |
| Arrays + Builtins        | ✅ Fertig  |
| String-Interpolation     | ✅ Fertig  |
| Ranges (.. / ...)        | ✅ Fertig  |
| Lambdas / Closures       | ✅ Fertig  |
| Async / Spawn / Await    | ✅ Fertig  |
| Channels + Select        | ✅ Fertig  |
| Try / Catch / Finally    | ✅ Fertig  |
| Modul / Import-System    | ✅ Fertig  |
| Float32 / Float64        | ✅ Fertig  |
| Map / Dict-Typ           | ⏳ Geplant |
| `defer`-Statement        | ⏳ Geplant |
| LSP (tinox-lsp)          | ✅ Fertig  |
| Eclipse Plugin           | ✅ Fertig  |
| REPL                     | ⏳ Geplant |

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
