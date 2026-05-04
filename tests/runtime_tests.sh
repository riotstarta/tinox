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

echo
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ ${#ERRORS[@]} -gt 0 ]; then
    echo "Failed: ${ERRORS[*]}"
    exit 1
fi
