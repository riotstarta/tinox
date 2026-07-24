# Tinox Programming Language

**Tinox** ist eine native, statisch typisierte Programmiersprache mit LLVM-Backend, Garbage Collection und Concurrency-Support.

> Benannt nach **Tino** + **Linux**/Unix – eine Sprache für moderne, lesbare Software.

## Status

**Phase:** V2 – Feature-Complete Core (In Development)

| Komponente   | Status      | Notizen                              |
|--------------|-------------|--------------------------------------|
| Lexer        | ✅ Fertig   | Unicode, String-Interpolation, Ranges |
| Parser       | ✅ Fertig   | Vollständiger AST                    |
| Type Checker | ✅ Fertig   | Basistypen, Klassen, Enums, Generics, Annotationen |
| Code Gen     | ✅ Fertig   | LLVM IR Backend, @inline-Unterstützung           |
| Runtime      | ✅ Fertig   | C Runtime (pthread-basiert)          |
| CLI          | ✅ Fertig   | build, run, check, fmt               |

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
tinox run program.tnx           # Kompilieren und ausführen
tinox build program.tnx         # Kompilieren (erzeugt ./a.out)
tinox check program.tnx         # Nur Type-Check
tinox fmt program.tnx           # Formatieren (Ausgabe auf stdout)
tinox fmt --write program.tnx   # Formatieren und Datei überschreiben
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

Instanzmethoden verwenden `fn` und haben Zugriff auf `this`. Konstruktoren werden mit `fnc new()` definiert:

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

Klassen mit Konstruktor:

```tinox
class Point
{
    x: Int64;
    y: Int64;

    fnc new(x: Int64, y: Int64) -> Point
    {
        return Point { x: x, y: y };
    }

    fn distanceTo(other: Point) -> Float64
    {
        let dx = this.x - other.x;
        let dy = this.y - other.y;
        return Math.sqrt((dx * dx + dy * dy).toFloat64());
    }
}

let p = Point::new(3, 4);
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

### Funktion-Typen

Funktionen als Werte und Parameter verwenden `fnc(T1, T2) -> R` als Typ:

```tinox
fn apply(x: Int64, f: fnc(Int64) -> Int64) -> Int64
{
    return f(x);
}

fn main() -> Int64
{
    let doubled = apply(21, n => n * 2);
    println(doubled);   // 42
    return 0;
}
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

// Typ-annotierte Map
let headers: Map<String, String> = Map::new();
headers["Content-Type"] = "application/json";
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
    let v = recv ch;
    println(v);
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

`throw` wirft eine Exception:

```tinox
fn divide(a: Int64, b: Int64) -> Int64
{
    if b == 0
    {
        throw "Division by zero";
    }
    return a / b;
}
```

### Annotations

Tinox unterstützt Annotations mit `@Name` oder `@Name(args)` Syntax auf Klassen, Methoden, Funktionen und Fields:

```tinox
@inline
fnc fastCalc(x: Int64) -> Int64
{
    return x * x + 1;
}

@deprecated("Bitte newApi verwenden")
fnc oldApi() -> Unit
{
    println("legacy");
}
```

Der Compiler validiert Annotations (unbekannte Annotations oder falsche Platzierung sind Fehler). Folgende Annotations werden erkannt:

| Annotation     | Targets              | Beschreibung                          |
|----------------|-----------------------|---------------------------------------|
| `@inline`      | Function, Method     | Erzeugt LLVM `alwaysinline`           |
| `@deprecated`  | Function, Method, Class | Warnung bei Nutzung                |
| `@GET`          | Method               | REST: GET-Endpunkt                    |
| `@POST`         | Method               | REST: POST-Endpunkt                   |
| `@PUT`          | Method               | REST: PUT-Endpunkt                    |
| `@PATCH`        | Method               | REST: PATCH-Endpunkt                  |
| `@DELETE`       | Method               | REST: DELETE-Endpunkt                 |
| `@Path`         | Class, Method        | URL-Pfad                              |
| `@Produces`    | Method               | Response Content-Type                 |
| `@Consumes`    | Method               | Erwarteter Request Content-Type        |
| `@StatusCode`  | Method               | Default HTTP-Statuscode               |
| `@Auth`         | Method, Class        | Authentifizierung ("bearer"/"basic")   |
| `@WebsocketEndpoint("/path"[, port])` | Class    | WebSocket: generiert einen Accept/Message-Loop als `main` |
| `@OnOpen`       | Method               | WebSocket: Aufruf bei neuer Verbindung |
| `@OnMessage`    | Method               | WebSocket: Aufruf pro Text-Nachricht   |
| `@OnClose`      | Method               | WebSocket: Aufruf beim Verbindungsende |

Beide Schreibweisen sind gleichwertig:

```tinox
@GET("/users")              // Pfad direkt in der Annotation
fnc listUsers(...) -> Unit { ... }

@GET                        // oder getrennt mit @Path
@Path("/users")
fnc listUsers(...) -> Unit { ... }
```

### HTTP Server & REST Framework

Die Standardbibliothek enthält einen HTTP-Server (`http_server`) und ein annotation-driven REST Framework. Das `mini_http`-Modul ist ein schlankes In-Process-Pendant:

```tinox
module mini_http;

class HttpRequest
{
    var method: String;
    var path: String;
    var body: String;
    var headers: Map<String, String>;
    var params: Map<String, String>;

    fnc new(method: String, path: String, ...) -> HttpRequest { ... }
}

class HttpResponse
{
    var statusCode: Int64;
    var headers: Map<String, String>;
    var body: String;

    fnc new() -> HttpResponse { ... }
}

class HttpServer
{
    var port: Int64;

    fnc new(port: Int64) -> HttpServer { ... }

    fn get(path: String, handler: fnc(HttpContext) -> Unit) -> HttpServer { ... }
    fn post(path: String, handler: fnc(HttpContext) -> Unit) -> HttpServer { ... }
    fn listen() -> Unit { ... }
}
```

REST-Controller mit Annotationen:

```tinox
import mini_http;

class UserController
{
    @GET
    @Path("/users")
    @Produces("application/json")
    @StatusCode(200)
    fnc listUsers(ctx: HttpContext) -> Unit
    {
        ctx.response.body = "[{\"id\":1,\"name\":\"Alice\"}]";
    }

    @POST
    @Path("/users")
    @Consumes("application/json")
    @StatusCode(201)
    fnc createUser(ctx: HttpContext) -> Unit
    {
        ctx.response.body = "{\"id\":2}";
    }

    @GET
    @Path("/users/:id")
    fnc getUser(ctx: HttpContext) -> Unit
    {
        let id: String = ctx.request.params["id"];
        ctx.response.body = "{\"id\":${id}}";
    }

    @DELETE
    @Path("/users/:id")
    @Auth("bearer")
    @StatusCode(204)
    fnc deleteUser(ctx: HttpContext) -> Unit
    {
        ctx.response.statusCode = 204;
    }
}
```

### WebSocket Server

Die Standardbibliothek enthält einen WebSocket-Server nach RFC 6455 (`websocket`), aufgebaut auf der Conn-Handle-Schicht des HTTP-Servers. v1 ist eine explizite Schleifen-API (kein Lambda-Handler), bedient eine Verbindung nach der anderen:

```tinox
import tinox.core.websocket;

let srv: Int64 = WsServer::listen(8790);

while true {
    let conn: Int64 = WsServer::accept(srv);   // inkl. Handshake
    if conn <= 0 { continue; }

    while true {
        let f: WsFrame = Ws::readMessage(conn); // Ping/Pong + Close automatisch
        if f.opcode == 1 {
            Ws::sendText(conn, "echo: " + Ws::text(f));
            continue;
        }
        break; // Close (8), EOF (-1) oder Protokollfehler (-2)
    }
    Ws::close(conn);
}
```

Bewusste v1-Lücken: keine Fragmentierung, kein Client, kein permessage-deflate.

`wss://` (TLS) wird ebenfalls unterstützt, per `WsServer::listenTls(port, certPath, keyPath)` + `WsServer::acceptTls(srv)` (sonst identische API). OpenSSL ist standardmäßig gelinkt, kein Extra-Flag nötig (Opt-out per `TINOX_TLS=0`, falls kein OpenSSL verfügbar ist):

```tinox
let srv = WsServer::listenTls(8791, "cert.pem", "key.pem");
let conn = WsServer::acceptTls(srv);   // inkl. TLS- + WS-Handshake
```

Alternativ annotation-getrieben (`@WebsocketEndpoint`/`@OnOpen`/`@OnMessage`/`@OnClose`): der Compiler generiert den kompletten Loop als `main` — kein Handshake/readMessage-Code nötig. Gilt nur, wenn die Datei kein eigenes `main` definiert und genau eine `@WebsocketEndpoint`-Klasse enthält (mehrere sind ein Compile-Fehler):

```tinox
import tinox.core.websocket;

@WebsocketEndpoint("/echo", 8793)
class EchoEndpoint
{
    @OnMessage
    fn onMessage(conn: Int64, msg: String) -> Nothing
    {
        Ws::sendText(conn, "echo: " + msg);
    }
}
```

## Feature-Übersicht

| Feature                      | Status     |
|------------------------------|------------|
| Variablen (let/var)          | ✅ Fertig  |
| Namespaces                   | ✅ Fertig  |
| Klassen + Vererbung          | ✅ Fertig  |
| Konstruktoren (`fnc new()`)  | ✅ Fertig  |
| Statische Methoden (`fnc`)   | ✅ Fertig  |
| Interfaces + vtable          | ✅ Fertig  |
| Enums + Pattern Matching     | ✅ Fertig  |
| Generics (Monomorphisierung) | ✅ Fertig  |
| Tuples                       | ✅ Fertig  |
| Arrays + Builtins            | ✅ Fertig  |
| String-Interpolation         | ✅ Fertig  |
| Ranges (.. / ...)            | ✅ Fertig  |
| Lambdas / Closures           | ✅ Fertig  |
| Funktion-Typen (`fnc(T)->R`) | ✅ Fertig  |
| Async / Spawn / Await        | ✅ Fertig  |
| Channels + Select            | ✅ Fertig  |
| Try / Catch / Finally        | ✅ Fertig  |
| `throw`-Statement            | ✅ Fertig  |
| Import-System                | ✅ Fertig  |
| Float32 / Float64            | ✅ Fertig  |
| Map / Dict-Typ               | ✅ Fertig  |
| `defer`-Statement            | ✅ Fertig  |
| Annotations                 | ✅ Fertig  |
| HTTP Server (stdlib)        | ✅ Fertig  |
| REST Framework (stdlib)     | ✅ Fertig  |
| WebSocket Server (stdlib)   | ✅ Fertig (v1) |
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
│   ├── tinox-typecheck/    # Type Checker + Annotation Processing
│   ├── tinox-codegen/      # LLVM IR Code Generation
│   ├── tinox-lsp/          # Language Server Protocol
│   ├── tinox/              # CLI Binary
│   └── tinox-core/         # Standardbibliothek (.tnx Module)
├── examples/               # Beispiel-Programme (.tnx)
└── runtime/                # C Runtime (tinox_alloc, threading, channels)
```

## Standardbibliothek (tinox-core)

50+ Module als `.tnx`-Dateien:

| Kategorie    | Module                                              |
|--------------|-----------------------------------------------------|
| HTTP         | `http_server`, `rest_framework`, `mini_http`, `websocket` |
| Daten        | `json`, `csv`, `xml`, `regex`                       |
| Sicherheit   | `crypto`, `jwt`, `bcrypt`                           |
| Collections  | `collections`, `queue`, `stack`, `linkedlist`       |
| System       | `fs`, `io`, `env`, `process`, `os`                  |
| Utilities    | `math`, `mathf`, `string_utils`, `date`, `uuid`     |
| Async        | `cron`, `events`, `pool`, `cache`, `pubsub`         |

## Lizenz

MIT OR Apache-2.0
