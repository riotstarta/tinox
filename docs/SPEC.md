# Tinox Programming Language Specification

**Version:** 0.1.0-dev  
**Status:** V1 Minimal - In Development

---

## Overview

Tinox is a statically typed, compiled programming language designed for readability and performance. It combines Java-like syntax with modern programming paradigms including concurrency primitives inspired by Go.

## Lexical Structure

### Identifiers

Identifiers start with a letter or underscore, followed by any number of letters, digits, or underscores.

```
identifier ::= (letter | '_') (letter | digit | '_')*
```

### Keywords

```
if          else        while       for         loop        return
break       continue    match       case        class       interface
extends     implements  trait       enum        new         this
super       self       public      private     protected   static
final       var        val         let         const       mut
fn          try        catch       finally     throw       spawn
channel     send       recv        async       await       module
import      export     as          in          where       extern
unsafe      ref        sizeof      typeof      null        true
false       Unit       Never       Any
```

### Literals

#### Integer Literals
```
int-literal ::= decimal | hex | octal | binary
decimal     ::= digit+
hex         ::= '0' ('x' | 'X') hexdigit+
octal       ::= '0' ('o' | 'O') octdigit+
binary      ::= '0' ('b' | 'B') bindigit+
```

#### Float Literals
```
float-literal ::= digit+ '.' digit* exponent?
exponent     ::= ('e' | 'E') ('+' | '-')? digit+
```

#### String Literals
```
string-literal ::= '"' (escaped | character)* '"'
escaped       ::= '\' (n | t | r | \ | " | ' | x hexdigit hexdigit | u '{' hexdigit+ '}' | U hexdigit{8})
```

#### Character Literals
```
char-literal ::= '\'' (escaped | character) '\''
```

#### Boolean Literals
```
true | false
```

### Operators

```
+    -    *    /    %    =    ==   !=   <    >    <=   >=
&&   ||   !    &    |    ^    ~    <<   >>   >>>  ++   --
+=   -=   *=   /=   %=   &=   |=   ^=   <<=  >>=  >>>=
->   =>   ::   ..   ...  ?    ??   ?:   @    ;    ,    .
```

### Whitespace and Comments

- Whitespace (spaces, tabs, carriage returns) is ignored except to separate tokens.
- Newlines separate statements.
- Single-line comments start with `//`.
- Multi-line comments start with `/*` and end with `*/`.

---

## Type System

### Primitive Types

| Type      | Description                     | Size    |
|-----------|--------------------------------|---------| 
| Int8      | 8-bit signed integer           | 1 byte  |
| Int16     | 16-bit signed integer         | 2 bytes |
| Int32     | 32-bit signed integer         | 4 bytes |
| Int64     | 64-bit signed integer         | 8 bytes |
| UInt8     | 8-bit unsigned integer        | 1 byte  |
| UInt16    | 16-bit unsigned integer       | 2 bytes |
| UInt32    | 32-bit unsigned integer       | 4 bytes |
| UInt64    | 64-bit unsigned integer       | 8 bytes |
| Float32   | 32-bit IEEE 754 float         | 4 bytes |
| Float64   | 64-bit IEEE 754 float         | 8 bytes |
| Bool      | Boolean (true/false)          | 1 byte  |
| Char      | Unicode scalar value          | 4 bytes |
| String    | UTF-8 encoded string          | varies  |
| Unit      | No value (like void)          | 0 bytes |
| Never     | Diverging type (never returns)| 0 bytes |
| Any       | Dynamic type (root of hierarchy)| varies |

### Composite Types

```
Array<T>     Fixed-size array of type T
Ref<T>       Mutable reference to T
Fn(P1, P2) -> R  Function type
```

---

## Declarations

### Variable Declarations

```tinox
// Immutable variable
let x: Int32 = 42;
val y: Int64 = 100;

// Mutable variable
var counter: Int32 = 0;
counter = counter + 1;
```

### Function Declarations

```tinox
fn greet(name: String) -> Unit {
    print("Hello, ");
    print(name);
    print("!");
}

fn add(a: Int32, b: Int32) -> Int32 {
    return a + b;
}

// Expression body (implicit return)
fn square(x: Int32) -> Int32 => x * x
```

### Class Declarations

```tinox
class Point {
    x: Float64;
    y: Float64;

    fn new(x: Float64, y: Float64) -> Point {
        return Point { x: x, y: y };
    }

    fn distanceTo(other: Point) -> Float64 {
        let dx: Float64 = this.x - other.x;
        let dy: Float64 = this.y - other.y;
        return (dx * dx + dy * dy).sqrt();
    }
}
```

### Class Inheritance

```tinox
class Shape {
    fn area() -> Float64 {
        return 0.0;
    }
}

class Circle extends Shape {
    radius: Float64;

    fn new(radius: Float64) -> Circle {
        return Circle { radius: radius };
    }

    override fn area() -> Float64 {
        return 3.14159 * this.radius * this.radius;
    }
}
```

### Interface Declarations

```tinox
interface Drawable {
    fn draw() -> Unit;
    fn getBounds() -> Rect;
}

class Canvas implements Drawable {
    fn draw() -> Unit {
        // ...
    }

    fn getBounds() -> Rect {
        return Rect { x: 0, y: 0, width: 100, height: 100 };
    }
}
```

### Enum Declarations

```tinox
enum Color {
    Red;
    Green;
    Blue;
    RGB(r: UInt8, g: UInt8, b: UInt8);
}

fn main() -> Int32 {
    let c: Color = Color.RGB(r: 255, g: 128, b: 0);
    match c {
        Red   => print("Red");
        Green => print("Green");
        Blue  => print("Blue");
        RGB(r, g, b) => print("Custom RGB");
    }
    return 0;
}
```

### Trait Declarations

```tinox
trait Printable {
    fn format() -> String;

    fn print() -> Unit {
        print(this.format());
    }
}
```

---

## Expressions

### Binary Operators (by precedence)

| Precedence | Operators                           | Associativity |
|------------|-------------------------------------|---------------|
| 1 (lowest)| ||                                 | Left          |
| 2         | &&                                 | Left          |
| 3         | ==  !=                             | Left          |
| 4         | <  <=  >  >=                       | Left          |
| 5         | <<  >>  >>>                        | Left          |
| 6         | +  -                               | Left          |
| 7         | *  /  %                            | Left          |
| 8 (highest)| unary -, !, ~                       | Right         |

### Control Flow

```tinox
// If-else
if x > 0 {
    print("positive");
} else if x < 0 {
    print("negative");
} else {
    print("zero");
}

// While loop
var i: Int32 = 0;
while i < 10 {
    print(i);
    i = i + 1;
}

// For loop
for item in collection {
    print(item);
}

// Loop (infinite)
loop {
    if done { break; }
}

// Match (pattern matching)
match value {
    0        => print("zero");
    1, 2     => print("one or two");
    n if n > 0 => print("positive");
    _        => print("other");
}
```

### Try-Catch

```tinox
try {
    riskyOperation();
} catch e: Error {
    print("Caught: ");
    print(e.message);
} finally {
    cleanup();
}
```

---

## Concurrency (V2)

### Channels

```tinox
let ch: Channel<Int32> = channel();

spawn(fn() {
    send(ch, 42);
});

let value: Int32 = recv(ch);
```

### Async/Await

```tinox
async fn fetchData(url: String) -> Data {
    // async operation
}

fn main() -> Unit {
    let data: Data = await fetchData("http://example.com");
    print(data);
}
```

---

## Standard Library (Planned)

### Core Types
- `Any` - Root type for all reference types
- `Unit` - Return type for procedures
- `Never` - Bottom type for diverging functions

### Collections
- `Array<T>` - Fixed-size array
- `List<T>` - Growable list
- `Map<K, V>` - Hash map
- `Set<T>` - Hash set

### Option/Result
- `Option<T>` - Optional value (Some/Tone)
- `Result<T, E>` - Error handling

---

## Grammar (EBNF)

```
source-file     ::= declaration*

declaration     ::= function
                  | class
                  | interface
                  | enum
                  | trait
                  | import

function        ::= 'fn' identifier '(' param-list? ')' '->' type block
                  | 'fn' identifier '(' param-list? ')' block

class           ::= 'class' identifier ('extends' type)? ('implements' type-list)? '{' class-body* '}'
class-body     ::= visibility? ('static'? ('fn' method | field-decl)) ';'

interface       ::= 'interface' identifier ('extends' type-list)? '{' fn-signature* '}'

enum            ::= 'enum' identifier '{' variant (',' variant)* '}'
variant         ::= identifier ('(' type-list? ')')?

trait           ::= 'trait' identifier '{' fn-signature* '}'

param-list      ::= param (',' param)*
param           ::= identifier ':' type

type            ::= 'Int8' | 'Int16' | 'Int32' | 'Int64'
                  | 'UInt8' | 'UInt16' | 'UInt32' | 'UInt64'
                  | 'Float32' | 'Float64' | 'Bool' | 'Char' | 'String'
                  | 'Unit' | 'Never' | 'Any'
                  | identifier
                  | 'Ref' '<' type '>'
                  | 'Array' '<' type '>'
                  | type '(' type-list? ')' '->' type

type-list       ::= type (',' type)*

visibility      ::= 'public' | 'private' | 'protected'

block           ::= '{' statement* '}'

statement       ::= 'let' identifier ':' type ('=' expression)? ';'
                  | 'var' identifier ':' type ('=' expression)? ';'
                  | 'if' expression block ('else' (block | 'if' ...))?
                  | 'while' expression block
                  | 'for' identifier 'in' expression block
                  | 'loop' block
                  | 'return' expression? ';'
                  | 'break' ';'
                  | 'continue' ';'
                  | 'throw' expression ';'
                  | 'try' block ('catch' '(' identifier ':' type ')' block)* ('finally' block)?
                  | block
                  | expression ';'
                  | ';'

expression      ::= assignment
assignment      ::= or-expr ('=' or-expr)*
or-expr         ::= and-expr ('||' and-expr)*
and-expr        ::= equality-expr ('&&' equality-expr)*
equality-expr   ::= relational-expr (('==' | '!=') relational-expr)*
relational-expr ::= shift-expr (('<' | '<=' | '>' | '>=') shift-expr)*
shift-expr      ::= additive-expr (('<<' | '>>' | '>>>') additive-expr)*
additive-expr   ::= multiplicative-expr (('+' | '-') multiplicative-expr)*
multiplicative-expr ::= unary-expr (('*' | '/' | '%') unary-expr)*
unary-expr      ::= ('-' | '!' | '~') unary-expr | postfix-expr
postfix-expr    ::= primary-expr ('(' args ')' | '.' identifier | '[' expression ']')*
primary-expr    ::= literal
                  | identifier
                  | 'this' | 'super'
                  | '(' expression ')'
                  | block
                  | 'if' expression 'then' expression ('else' expression)?
                  | 'match' expression '{' match-case* '}'

args            ::= expression (',' expression)*
match-case      ::= pattern ('if' expression)? '=>' expression ','
pattern         ::= '_' | identifier | literal | pattern ',' pattern
```

---

## Implementation Status

| Component    | Status    | Notes                           |
|--------------|-----------|---------------------------------|
| Lexer        | ✅ Done   | Full Unicode support            |
| Parser       | ✅ Done   | AST generation                  |
| Type Checker | ⚠️ Placeholder | Pass-through for V1         |
| Code Gen     | 🔄 In Progress | LLVM backend               |
| Runtime      | ✅ Done   | C runtime                       |
| Standard Lib | ⏳ Planned | V2+                           |
| Concurrency  | ⏳ Planned | Channels, async/await          |
| Tooling      | ⏳ Planned | REPL, LSP, Debugger           |

---

## Future Features

- [ ] Generics with bounds
- [ ] Pattern matching exhaustiveness
- [ ] Channels and goroutine-style concurrency
- [ ] Async/await
- [ ] Traits with default implementations
- [ ] Macros
- [ ] REPL
- [ ] Language Server Protocol (LSP)
- [ ] Debugger support
