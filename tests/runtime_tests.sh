#!/usr/bin/env bash
# Runtime tests for the Tinox compiler
set -uo pipefail
TINOX="./target/release/tinox"
PASS=0
FAIL=0
ERRORS=()

run_test() {
    local name="$1"
    local file="$2"
    local expected="$3"
    local actual
    actual=$(timeout 10 $TINOX run "$file" 2>&1) || { actual="CRASH/TIMEOUT"; }
    if [ "$actual" = "$expected" ]; then
        echo "  PASS  $name"
        ((PASS++))
    else
        echo "  FAIL  $name"
        echo "        expected: $(echo "$expected" | head -3)"
        echo "        got:      $(echo "$actual"   | head -3)"
        ((FAIL++))
        ERRORS+=("$name")
    fi
}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "=== Tinox Runtime Tests ==="
echo

# ── Test 1: Hello World ─────────────────────────────────────
cat >"$TMP/t1.tnx" <<'EOF'
fn main() -> Int32
{
    println("Hello, World!");
    return 0;
}
EOF
run_test "hello_world" "$TMP/t1.tnx" "Hello, World!"

# ── Test 2: Arithmetic ──────────────────────────────────────
cat >"$TMP/t2.tnx" <<'EOF'
fn main() -> Int32
{
    let a: Int64 = 6;
    let b: Int64 = 7;
    println(a * b);
    return 0;
}
EOF
run_test "arithmetic_mul" "$TMP/t2.tnx" "42"

# ── Test 3: Factorial (recursion) ───────────────────────────
cat >"$TMP/t3.tnx" <<'EOF'
fn factorial(n: Int64) -> Int64
{
    if n <= 1 { return 1; }
    return n * factorial(n - 1);
}
fn main() -> Int32
{
    println(factorial(10));
    return 0;
}
EOF
run_test "factorial_recursion" "$TMP/t3.tnx" "3628800"

# ── Test 4: Fibonacci iterative ─────────────────────────────
cat >"$TMP/t4.tnx" <<'EOF'
fn fib(n: Int64) -> Int64
{
    var a: Int64 = 0;
    var b: Int64 = 1;
    var i: Int64 = 0;
    while i < n
    {
        let t: Int64 = b;
        b = a + b;
        a = t;
        i = i + 1;
    }
    return a;
}
fn main() -> Int32
{
    println(fib(20));
    return 0;
}
EOF
run_test "fibonacci_iterative" "$TMP/t4.tnx" "6765"

# ── Test 5: String concatenation ────────────────────────────
cat >"$TMP/t5.tnx" <<'EOF'
fn main() -> Int32
{
    let s: String = "foo" + "bar" + "baz";
    println(s);
    return 0;
}
EOF
run_test "string_concat" "$TMP/t5.tnx" "foobarbaz"

# ── Test 6: While + counter ─────────────────────────────────
cat >"$TMP/t6.tnx" <<'EOF'
fn main() -> Int32
{
    var sum: Int64 = 0;
    var i: Int64 = 1;
    while i <= 100
    {
        sum = sum + i;
        i = i + 1;
    }
    println(sum);
    return 0;
}
EOF
run_test "sum_1_to_100" "$TMP/t6.tnx" "5050"

# ── Test 7: If/else chain ───────────────────────────────────
cat >"$TMP/t7.tnx" <<'EOF'
fn classify(n: Int64) -> String
{
    if n < 0 { return "negative"; }
    else if n == 0 { return "zero"; }
    else { return "positive"; }
}
fn main() -> Int32
{
    println(classify(-5));
    println(classify(0));
    println(classify(42));
    return 0;
}
EOF
run_test "if_else_chain" "$TMP/t7.tnx" "$(printf 'negative\nzero\npositive')"

# ── Test 8: List push/len/index ─────────────────────────────
cat >"$TMP/t8.tnx" <<'EOF'
fn main() -> Int32
{
    var nums: List<Int64> = [];
    nums.push(10);
    nums.push(20);
    nums.push(30);
    println(nums.len());
    println(nums[1]);
    return 0;
}
EOF
run_test "list_push_index" "$TMP/t8.tnx" "$(printf '3\n20')"

# ── Test 9: Map set/get ─────────────────────────────────────
cat >"$TMP/t9.tnx" <<'EOF'
fn main() -> Int32
{
    var m: Map<String, Int64> = Map::new();
    m["one"] = 1;
    m["two"] = 2;
    println(m["one"] + m["two"]);
    return 0;
}
EOF
run_test "map_set_get" "$TMP/t9.tnx" "3"

# ── Test 10: Struct / class ─────────────────────────────────
cat >"$TMP/t10.tnx" <<'EOF'
class Point
{
    x: Int64;
    y: Int64;
}
fn distance(p: Point) -> Int64
{
    return p.x * p.x + p.y * p.y;
}
fn main() -> Int32
{
    let p: Point = Point { x: 3, y: 4 };
    println(distance(p));
    return 0;
}
EOF
run_test "struct_field_access" "$TMP/t10.tnx" "25"

# ── Test 11: Static method (fnc) ────────────────────────────
cat >"$TMP/t11.tnx" <<'EOF'
class MathUtil
{
    fnc square(n: Int64) -> Int64
    {
        return n * n;
    }
    fnc cube(n: Int64) -> Int64
    {
        return n * n * n;
    }
}
fn main() -> Int32
{
    println(MathUtil::square(5));
    println(MathUtil::cube(3));
    return 0;
}
EOF
run_test "static_methods" "$TMP/t11.tnx" "$(printf '25\n27')"

# ── Test 12: Closures / lambdas ─────────────────────────────
cat >"$TMP/t12.tnx" <<'EOF'
fn apply(f: fnc(Int64) -> Int64, x: Int64) -> Int64
{
    return f(x);
}
fn main() -> Int32
{
    let double: fnc(Int64) -> Int64 = x => x * 2;
    println(apply(double, 21));
    return 0;
}
EOF
run_test "lambda_higher_order" "$TMP/t12.tnx" "42"

# ── Test 13: For-in range ───────────────────────────────────
cat >"$TMP/t13.tnx" <<'EOF'
fn main() -> Int32
{
    var sum: Int64 = 0;
    for i in 1..6
    {
        sum = sum + i;
    }
    println(sum);
    return 0;
}
EOF
run_test "for_range" "$TMP/t13.tnx" "15"

# ── Test 14: String methods ─────────────────────────────────
cat >"$TMP/t14.tnx" <<'EOF'
fn main() -> Int32
{
    let s: String = "Hello, World!";
    println(s.len());
    println(s.substring(7, 12));
    return 0;
}
EOF
run_test "string_methods" "$TMP/t14.tnx" "$(printf '13\nWorld')"

# ── Test 15: Bitwise ops + shift ────────────────────────────
cat >"$TMP/t15.tnx" <<'EOF'
fn main() -> Int32
{
    let a: Int64 = 0xFF;
    let b: Int64 = a & 0x0F;
    let c: Int64 = 1 << 8;
    println(b);
    println(c);
    return 0;
}
EOF
run_test "bitwise_ops" "$TMP/t15.tnx" "$(printf '15\n256')"

# ── Test 16: Nested functions + multiple returns ─────────────
cat >"$TMP/t16.tnx" <<'EOF'
fn abs_val(n: Int64) -> Int64
{
    if n < 0 { return -n; }
    return n;
}
fn max_val(a: Int64, b: Int64) -> Int64
{
    if a > b { return a; }
    return b;
}
fn main() -> Int32
{
    println(abs_val(-99));
    println(max_val(13, 37));
    return 0;
}
EOF
run_test "abs_max" "$TMP/t16.tnx" "$(printf '99\n37')"

# ── Test 17: Float arithmetic ───────────────────────────────
cat >"$TMP/t17.tnx" <<'EOF'
fn main() -> Int32
{
    let x: Float64 = 1.5;
    let y: Float64 = 2.5;
    println(x + y);
    return 0;
}
EOF
run_test "float_arithmetic" "$TMP/t17.tnx" "4"

# ── Test 18: Modulo / GCD ───────────────────────────────────
cat >"$TMP/t18.tnx" <<'EOF'
fn gcd(a: Int64, b: Int64) -> Int64
{
    var x: Int64 = a;
    var y: Int64 = b;
    while y != 0
    {
        let t: Int64 = y;
        y = x % y;
        x = t;
    }
    return x;
}
fn main() -> Int32
{
    println(gcd(48, 18));
    return 0;
}
EOF
run_test "gcd_modulo" "$TMP/t18.tnx" "6"

# ── Test 19: Break / continue ───────────────────────────────
cat >"$TMP/t19.tnx" <<'EOF'
fn main() -> Int32
{
    var n: Int64 = 0;
    var i: Int64 = 0;
    while i < 20
    {
        i = i + 1;
        if i % 2 == 0 { continue; }
        if i > 9 { break; }
        n = n + i;
    }
    println(n);
    return 0;
}
EOF
run_test "break_continue" "$TMP/t19.tnx" "25"

# ── Test 20: Cast as ────────────────────────────────────────
cat >"$TMP/t20.tnx" <<'EOF'
fn main() -> Int32
{
    let x: Float64 = 3.9;
    let y: Int64 = x as Int64;
    println(y);
    return 0;
}
EOF
run_test "cast_as" "$TMP/t20.tnx" "3"

# ── Test 21: HPACK encodeInt — RFC 7541 Appendix C.1.3 ──────
cat >"$TMP/t21.tnx" <<'EOF'
import tinox.core.hpack;
fn main() -> Int32
{
    // Encode integer 10 with 5-bit prefix → single byte 10
    let r1: List<Int64> = Hpack::encodeInt(10, 5, 0);
    println(r1.len());
    println(r1[0]);
    // Encode integer 1337 with 5-bit prefix → [31, 154, 10] per RFC 7541 §C.1.3
    let r2: List<Int64> = Hpack::encodeInt(1337, 5, 0);
    println(r2.len());
    println(r2[0]);
    println(r2[1]);
    println(r2[2]);
    return 0;
}
EOF
run_test "hpack_encode_int" "$TMP/t21.tnx" "$(printf '1\n10\n3\n31\n154\n10')"

# ── Test 22: HPACK decodeInt ─────────────────────────────────
cat >"$TMP/t22.tnx" <<'EOF'
import tinox.core.hpack;
fn main() -> Int32
{
    // Decode integer 1337 from bytes [31, 154, 10] with 5-bit prefix
    let data: List<Int64> = [31, 154, 10];
    let result: HpackIntResult = Hpack::decodeInt(data, 0, 5);
    println(result.value);
    println(result.nextOffset);
    // Decode small integer 10 from single byte [10] with 5-bit prefix
    let data2: List<Int64> = [10];
    let r2: HpackIntResult = Hpack::decodeInt(data2, 0, 5);
    println(r2.value);
    println(r2.nextOffset);
    return 0;
}
EOF
run_test "hpack_decode_int" "$TMP/t22.tnx" "$(printf '1337\n3\n10\n1')"

# ── Test 23: HPACK static table ──────────────────────────────
cat >"$TMP/t23.tnx" <<'EOF'
import tinox.core.hpack;
fn main() -> Int32
{
    println(Hpack::staticName(2));
    println(Hpack::staticValue(2));
    println(Hpack::staticName(8));
    println(Hpack::staticValue(8));
    println(Hpack::staticName(28));
    println(Hpack::staticName(61));
    return 0;
}
EOF
run_test "hpack_static_table" "$TMP/t23.tnx" "$(printf ':method\nGET\n:status\n200\ncontent-length\nwww-authenticate')"

# ── Test 24: HPACK encode/decode round-trip ──────────────────
cat >"$TMP/t24.tnx" <<'EOF'
import tinox.core.hpack;
fn main() -> Int32
{
    let dynEnc: HpackDynTable = HpackDynTable::new(4096);
    let dynDec: HpackDynTable = HpackDynTable::new(4096);
    let headers: List<HpackHeader> = [];
    headers.push(HpackHeader::new(":status", "200"));
    headers.push(HpackHeader::new("content-type", "application/json; charset=utf-8"));
    headers.push(HpackHeader::new("x-request-id", "abc-123"));
    let encoded: List<Int64> = Hpack::encode(headers, dynEnc);
    let decoded: List<HpackHeader> = Hpack::decode(encoded, dynDec);
    println(decoded.len());
    println(decoded[0].name);
    println(decoded[0].value);
    println(decoded[1].name);
    println(decoded[2].name);
    println(decoded[2].value);
    return 0;
}
EOF
run_test "hpack_roundtrip" "$TMP/t24.tnx" "$(printf '3\n:status\n200\ncontent-type\nx-request-id\nabc-123')"

# ── Test 25: HPACK dynamic table is populated on encode ──────
cat >"$TMP/t25.tnx" <<'EOF'
import tinox.core.hpack;
fn main() -> Int32
{
    let dynEnc: HpackDynTable = HpackDynTable::new(4096);
    let headers: List<HpackHeader> = [];
    headers.push(HpackHeader::new("x-token", "secret"));
    headers.push(HpackHeader::new("x-tenant", "acme"));
    let encoded: List<Int64> = Hpack::encode(headers, dynEnc);
    // Both non-static headers should be added to the dynamic table
    println(dynEnc.entries.len());
    println(dynEnc.entries[0].name);
    println(dynEnc.entries[0].value);
    println(dynEnc.entries[1].name);
    return 0;
}
EOF
run_test "hpack_dyn_table" "$TMP/t25.tnx" "$(printf '2\nx-tenant\nacme\nx-token')"

# ── Test 26: HTTP/2 frame header byte serialization ──────────
cat >"$TMP/t26.tnx" <<'EOF'
fn encodeFrameHeader(length: Int64, frameType: Int64, flags: Int64, streamId: Int64) -> List<Int64>
{
    let out: List<Int64> = [];
    out.push((length >> 16) & 255);
    out.push((length >> 8) & 255);
    out.push(length & 255);
    out.push(frameType & 255);
    out.push(flags & 255);
    out.push((streamId >> 24) & 127);
    out.push((streamId >> 16) & 255);
    out.push((streamId >> 8) & 255);
    out.push(streamId & 255);
    return out;
}
fn main() -> Int32
{
    // SETTINGS frame: length=12, type=0x04, flags=0, stream=0
    let h1: List<Int64> = encodeFrameHeader(12, 4, 0, 0);
    println(h1.len());
    println(h1[2]);
    println(h1[3]);
    // HEADERS frame: length=256, type=0x01, flags=0x05 (END_HEADERS|END_STREAM), stream=1
    let h2: List<Int64> = encodeFrameHeader(256, 1, 5, 1);
    println(h2[0]);
    println(h2[1]);
    println(h2[2]);
    println(h2[3]);
    println(h2[4]);
    println(h2[8]);
    // Large stream ID: stream=0x00ABCDEF
    let h3: List<Int64> = encodeFrameHeader(0, 0, 0, 11259375);
    println(h3[5]);
    println(h3[6]);
    println(h3[7]);
    println(h3[8]);
    return 0;
}
EOF
run_test "http2_frame_header_bytes" "$TMP/t26.tnx" "$(printf '9\n12\n4\n0\n1\n0\n1\n5\n1\n0\n171\n205\n239')"

# ── ORM / SQLite Tests ───────────────────────────────────────
# run_db_test <name> <tnx_code> <setup_sql> <expected>
# Creates an isolated dir with tinox.toml + SQLite DB, cds in, runs tinox.
ABS_TINOX="$(cd "$(dirname "$TINOX")" && pwd)/$(basename "$TINOX")"

run_db_test() {
    local name="$1"
    local tnx_code="$2"
    local setup_sql="$3"
    local expected="$4"

    if ! command -v sqlite3 &>/dev/null; then
        echo "  SKIP  $name (sqlite3 not installed)"
        return
    fi

    local dir="$TMP/db_$name"
    mkdir -p "$dir"
    cat >"$dir/tinox.toml" <<'TOML'
[database]
driver = "sqlite"
url = "test.db"
TOML
    printf '%s\n' "$tnx_code" >"$dir/test.tnx"
    sqlite3 "$dir/test.db" "$setup_sql"

    local actual
    actual=$(cd "$dir" && timeout 15 "$ABS_TINOX" run test.tnx 2>&1) || { actual="CRASH/TIMEOUT"; }
    if [ "$actual" = "$expected" ]; then
        echo "  PASS  $name"
        ((PASS++))
    else
        echo "  FAIL  $name"
        echo "        expected: $(echo "$expected" | head -5)"
        echo "        got:      $(echo "$actual"   | head -5)"
        ((FAIL++))
        ERRORS+=("$name")
    fi
}

# ── Test 27: ORM — list() alle Zeilen ───────────────────────
read -r -d '' TNX_27 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("users")
class User
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("age")
    var age: Int64;
}

fn main() -> Int32
{
    let users: List<User> = DB.of(User).list();
    println(users.len());
    println(users[0].name);
    return 0;
}
EOF
run_db_test "orm_sqlite_list" "$TNX_27" \
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INTEGER NOT NULL);
     INSERT INTO users (name, age) VALUES ('Alice', 30), ('Bob', 25), ('Carol', 35);" \
    "$(printf '3\nAlice')"

# ── Test 28: ORM — filter(u -> u.age > 25) ──────────────────
read -r -d '' TNX_28 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("users")
class User
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("age")
    var age: Int64;
}

fn main() -> Int32
{
    let adults: List<User> = DB.of(User)
        .filter(u => u.age > 25)
        .list();
    println(adults.len());
    println(adults[0].name);
    return 0;
}
EOF
run_db_test "orm_sqlite_filter_gt" "$TNX_28" \
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INTEGER NOT NULL);
     INSERT INTO users (name, age) VALUES ('Alice', 30), ('Bob', 20), ('Carol', 35);" \
    "$(printf '2\nAlice')"

# ── Test 29: ORM — count() ──────────────────────────────────
read -r -d '' TNX_29 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("products")
class Product
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("price")
    var price: Int64;
}

fn main() -> Int32
{
    let total: Int64 = DB.of(Product).count();
    println(total);
    let cheap: Int64 = DB.of(Product)
        .filter(p => p.price < 100)
        .count();
    println(cheap);
    return 0;
}
EOF
run_db_test "orm_sqlite_count" "$TNX_29" \
    "CREATE TABLE products (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, price INTEGER NOT NULL);
     INSERT INTO products (name, price) VALUES ('Widget', 50), ('Gadget', 150), ('Doohickey', 75);" \
    "$(printf '3\n2')"

# ── Test 30: ORM — orderBy + limit ──────────────────────────
read -r -d '' TNX_30 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("users")
class User
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("age")
    var age: Int64;
}

fn main() -> Int32
{
    let top2: List<User> = DB.of(User)
        .orderBy(u => u.name)
        .limit(2)
        .list();
    println(top2.len());
    println(top2[0].name);
    println(top2[1].name);
    return 0;
}
EOF
run_db_test "orm_sqlite_order_limit" "$TNX_30" \
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INTEGER NOT NULL);
     INSERT INTO users (name, age) VALUES ('Zara', 28), ('Alice', 30), ('Bob', 25), ('Carol', 35);" \
    "$(printf '2\nAlice\nBob')"

# ── Test 31: ORM — first() mit filter ───────────────────────
read -r -d '' TNX_31 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("users")
class User
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("age")
    var age: Int64;
}

fn main() -> Int32
{
    let u: User = DB.of(User)
        .filter(u => u.name == "Carol")
        .first();
    println(u.name);
    println(u.age);
    return 0;
}
EOF
run_db_test "orm_sqlite_first" "$TNX_31" \
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INTEGER NOT NULL);
     INSERT INTO users (name, age) VALUES ('Alice', 30), ('Bob', 25), ('Carol', 35);" \
    "$(printf 'Carol\n35')"

# ── Test 32: ORM — filter mit String startsWith ─────────────
read -r -d '' TNX_32 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("users")
class User
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("name")
    var name: String;
    @Column("age")
    var age: Int64;
}

fn main() -> Int32
{
    let cs: List<User> = DB.of(User)
        .filter(u => u.name.startsWith("C"))
        .list();
    println(cs.len());
    println(cs[0].name);
    return 0;
}
EOF
run_db_test "orm_sqlite_filter_startswith" "$TNX_32" \
    "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INTEGER NOT NULL);
     INSERT INTO users (name, age) VALUES ('Alice', 30), ('Carol', 35), ('Charlie', 22);" \
    "$(printf '2\nCarol')"

# ── Test 33: ORM — orderByDesc + offset ─────────────────────
read -r -d '' TNX_33 <<'EOF' || true
import tinox.core.db;

@Entity
@Table("scores")
class Score
{
    @Id @GeneratedValue
    var id: Int64;
    @Column("player")
    var player: String;
    @Column("points")
    var points: Int64;
}

fn main() -> Int32
{
    let ranked: List<Score> = DB.of(Score)
        .orderByDesc(s => s.points)
        .offset(1)
        .limit(2)
        .list();
    println(ranked.len());
    println(ranked[0].player);
    println(ranked[1].player);
    return 0;
}
EOF
run_db_test "orm_sqlite_order_desc_offset" "$TNX_33" \
    "CREATE TABLE scores (id INTEGER PRIMARY KEY AUTOINCREMENT, player TEXT NOT NULL, points INTEGER NOT NULL);
     INSERT INTO scores (player, points) VALUES ('Alice', 100), ('Bob', 300), ('Carol', 200), ('Dave', 150);" \
    "$(printf '2\nCarol\nDave')"

echo
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ ${#ERRORS[@]} -gt 0 ]; then
    echo "Failed: ${ERRORS[*]}"
    exit 1
fi
