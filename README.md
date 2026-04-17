# Tinox Programming Language

**Tinox** ist eine native, statisch typisierte Programmiersprache mit Garbage Collection und Concurrency-Support.

> Benannt nach **Tino** + **Linux**/Unix - eine Sprache für moderne, lesbare Software.

## Status

**Phase:** V1 Minimal (In Development)

| Component    | Status    | Notes                           |
|--------------|-----------|---------------------------------|
| Lexer        | ✅ Done   | Full Unicode support            |
| Parser       | ✅ Done   | AST generation                  |
| Type Checker | ⚠️ Placeholder | Pass-through for V1         |
| Code Gen     | 🔄 In Progress | LLVM backend               |
| Runtime      | ✅ Done   | C runtime                       |
| CLI          | 🔄 In Progress | build, run, check           |

## Design-Ziele

- **Lesbare Syntax** - Java-ähnlich, explizite Typen
- **Performant** - LLVM Backend für nativen Maschinencode
- **Sicher** - Null-Safety, Bounds Checking
- **Concurrent** - Goroutine-ähnliches Concurrency-Modell mit Channels
- **Modern** - Generics, Traits, Pattern Matching, Async/Await

## Installation

```bash
# Clone the repository
git clone https://github.com/your-repo/tinox.git
cd tinox

# Build the compiler
cargo build --release

# The binary will be at target/release/tinox
```

## Usage

```bash
# Check (type check without compiling)
./target/release/tinox check program.tnx

# Build
./target/release/tinox build program.tnx -o myprogram

# Run
./target/release/tinox run program.tnx
```

## Hello World

```tinox
fn main() -> Int32 {
    print("Hello, World!");
    return 0;
}
```

## Example Code

```tinox
// Functions
fn add(a: Int32, b: Int32) -> Int32 {
    return a + b;
}

// Classes
class Point {
    x: Float64;
    y: Float64;
    
    fn new(x: Float64, y: Float64) -> Point {
        return Point { x: x, y: y };
    }
}

// Enums with pattern matching
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

## Projekt-Struktur

```
tinox/
├── Cargo.toml              # Workspace configuration
├── crates/
│   ├── tinox-common/       # Shared types (Span, Error)
│   ├── tinox-lexer/       # Lexer/Tokenizer
│   ├── tinox-parser/      # Parser + AST
│   ├── tinox-typecheck/   # Type checker
│   ├── tinox-codegen/     # LLVM code generation
│   └── tinox/             # Main CLI binary
├── runtime/                # C runtime library
├── docs/                   # Language specification
└── examples/              # Example programs
```

## Sprache-Features (geplant)

| Feature              | Status   |
|----------------------|----------|
| Variablen (let/var)  | ✅ Done   |
| Funktionen           | ✅ Done   |
| Klassen              | ✅ Done   |
| Vererbung            | ✅ Done   |
| Interfaces           | ✅ Done   |
| Enums                | ✅ Done   |
| Pattern Matching      | ✅ Done   |
| Generics             | ⏳ Planned |
| Traits               | ⏳ Planned |
| Channels             | ⏳ Planned |
| Async/Await          | ⏳ Planned |
| Garbage Collection    | ⏳ Planned |
| REPL                 | ⏳ Planned |
| LSP                  | ⏳ Planned |

## Development

### Running Tests

```bash
cargo test
```

### Building for Release

```bash
cargo build --release
```

### Checking Dependencies

```bash
cargo check
```

## Dokumentation

- [Sprach-Spezifikation](docs/SPEC.md) - Vollständige Sprachreferenz
- [.tinox_progress](.tinox_progress) - Entwicklungsfortschritt

## Contributing

(TBD)

## Lizenz

MIT OR Apache-2.0
