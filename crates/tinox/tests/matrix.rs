//! Kontext-Matrix (TESTPLAN Phase 1.3).
//!
//! Schleust dieselben Kern-Operationen pro Typ durch alle Herkunfts-Kontexte
//! und verlangt identisches Verhalten. Die .tnx-Dateien werden zur Laufzeit
//! generiert (nichts eingecheckt); pro (Typ × Kontext) entsteht eine Datei
//! mit allen Operationen dieses Typs.
//!
//! Bekannte Löcher stehen in KNOWN_FAILURES: ein dort gelisteter Fall MUSS
//! fehlschlagen (sonst ist der Eintrag veraltet und der Test rot), ein nicht
//! gelisteter Fall MUSS bestehen. Fixes erzwingen also das Pflegen der Liste.

mod common;
use common::{parse_case, run_case};
use std::fs;
use std::path::PathBuf;

/// Kontexte, die heute noch fehlschlagen — jeder Eintrag ist ein offenes
/// Inferenz-Loch (Format: "matrix_<typ>_<kontext>"). Beim Fixen: Eintrag
/// entfernen, sonst schlägt der Test mit "stale entry" fehl.
const KNOWN_FAILURES: &[&str] = &[];

struct TypeSpec {
    key: &'static str,
    /// Tinox-Typname (für Annotationen, Parameter, Felder, Payloads)
    tnx: &'static str,
    /// Literal-Ausdruck des Referenzwerts
    lit: &'static str,
    /// Anderes Literal gleichen Typs (Erstinitialisierung vor Neuzuweisung)
    other: &'static str,
    /// Operationen: Statement-Template mit {V} als Wert-Ausdruck + erwartete Zeilen
    ops: &'static [(&'static str, &'static [&'static str])],
}

const TYPES: &[TypeSpec] = &[
    TypeSpec {
        key: "string",
        tnx: "String",
        lit: r#""hello""#,
        other: r#""zz""#,
        ops: &[
            ("println({V}.len());", &["5"]),
            ("println({V});", &["hello"]),
            (r#"println({V} + "!");"#, &["hello!"]),
            (r#"if {V} == "hello" { println("eq"); } else { println("ne"); }"#, &["eq"]),
            (r#"if {V}.contains("ell") { println("has"); } else { println("not"); }"#, &["has"]),
            ("println({V}.substring(1, 3));", &["el"]),
        ],
    },
    TypeSpec {
        key: "listint",
        tnx: "List<Int64>",
        lit: "[10, 20, 30]",
        other: "[7]",
        ops: &[
            ("println({V}.len());", &["3"]),
            ("println({V}[1]);", &["20"]),
            ("var s = 0;\n    for x in {V} { s += x; }\n    println(s);", &["60"]),
            (r#"if {V}.contains(20) { println("has"); } else { println("not"); }"#, &["has"]),
            ("println({V}.first());", &["10"]),
            ("println({V}.last());", &["30"]),
        ],
    },
    TypeSpec {
        key: "liststr",
        tnx: "List<String>",
        lit: r#"["ab", "cde"]"#,
        other: r#"["z"]"#,
        ops: &[
            ("println({V}.len());", &["2"]),
            ("println({V}[0]);", &["ab"]),
            ("println({V}[1].len());", &["3"]),
            ("for x in {V} { println(x.len()); }", &["2", "3"]),
        ],
    },
    TypeSpec {
        key: "float",
        tnx: "Float64",
        lit: "1.5",
        other: "9.0",
        ops: &[
            ("println({V}.toString());", &["1.5"]),
            ("println(({V} + 1.25).toString());", &["2.75"]),
            (r#"if {V} < 2.0 { println("lt"); } else { println("ge"); }"#, &["lt"]),
        ],
    },
    TypeSpec {
        key: "listfloat",
        tnx: "List<Float64>",
        lit: "[1.5, 2.25]",
        other: "[9.0]",
        ops: &[
            ("println({V}.len());", &["2"]),
            ("println({V}[1].toString());", &["2.25"]),
            ("var s = 0.0;\n    for x in {V} { s += x; }\n    println(s.toString());", &["3.75"]),
        ],
    },
];

/// Wie der Wert an die Operation kommt. Liefert (Präambel-Decls,
/// Setup-Statements in main, Wert-Ausdruck) — oder None, wenn der Kontext
/// für den Typ nicht sinnvoll ist.
fn apply_context(ctx: &str, ty: &TypeSpec) -> Option<(String, String, String)> {
    let t = ty.tnx;
    let lit = ty.lit;
    let other = ty.other;
    Some(match ctx {
        // Operation direkt auf dem Literal
        "literal" => (String::new(), String::new(), lit.to_string()),
        // let ohne Typannotation
        "let" => (String::new(), format!("let v = {lit};"), "v".into()),
        // let mit Typannotation
        "let_ann" => (String::new(), format!("let v: {t} = {lit};"), "v".into()),
        // var, danach Neuzuweisung aus Funktionsergebnis
        "var_reassign" => (
            format!("fn make() -> {t} {{\n    return {lit};\n}}\n"),
            format!("var v: {t} = {other};\n    v = make();"),
            "v".into(),
        ),
        // Funktionsparameter
        "param" => (
            String::new(),
            String::new(),
            // Sonderfall: Ops laufen in einer eigenen Funktion, siehe emit_case
            "v".into(),
        ),
        // let aus Funktionsrückgabe
        "let_from_fn" => (
            format!("fn make() -> {t} {{\n    return {lit};\n}}\n"),
            "let v = make();".to_string(),
            "v".into(),
        ),
        // Operation direkt auf dem Call-Ausdruck
        "ret_direct" => (
            format!("fn make() -> {t} {{\n    return {lit};\n}}\n"),
            String::new(),
            "make()".into(),
        ),
        // Element einer Liste (Literal)
        "list_elem" => (
            String::new(),
            format!("let xs: List<{t}> = [{lit}];"),
            "xs[0]".into(),
        ),
        // Element einer Liste aus Funktionsrückgabe
        "list_elem_fn" => (
            format!("fn makeList() -> List<{t}> {{\n    return [{lit}];\n}}\n"),
            "let xs = makeList();".to_string(),
            "xs[0]".into(),
        ),
        // Klassenfeld
        "field" => (
            format!("class Holder {{\n    var f: {t};\n}}\n"),
            format!("let h = Holder {{ f: {lit} }};"),
            "h.f".into(),
        ),
        // Element eines List-Felds
        "field_list_elem" => (
            format!("class Holder {{\n    var xs: List<{t}>;\n}}\n"),
            format!("let h = Holder {{ xs: [{lit}] }};"),
            "h.xs[0]".into(),
        ),
        // Match-Payload
        "match_payload" => (
            format!("enum Box {{\n    Val({t}),\n    Empty,\n}}\n"),
            format!("let b = Box::Val({lit});"),
            // Sonderfall: Ops laufen im Match-Arm, siehe emit_case
            "v".into(),
        ),
        // Schleifenvariable (Liste des Typs), nur 1 Element
        "loop_var" => (
            String::new(),
            format!("let xs: List<{t}> = [{lit}];"),
            // Sonderfall: Ops laufen im Schleifenkörper
            "v".into(),
        ),
        // Map-Value: Wert steckt als Value in einer Map<String, T>
        "map_value" => (
            String::new(),
            format!(
                "var m: Map<String, {t}> = Map::new();\n    m.insert(\"k\", {lit});"
            ),
            r#"m.get("k")"#.into(),
        ),
        // Cross-Modul: Wert kommt aus einer Funktion eines anderen Moduls
        "cross_module" => (
            "import _matrix_mod;\n".to_string(),
            format!("let v = mk_{}();", ty.key),
            "v".into(),
        ),
        _ => return None,
    })
}

const CONTEXTS: &[&str] = &[
    "literal",
    "let",
    "let_ann",
    "var_reassign",
    "param",
    "let_from_fn",
    "ret_direct",
    "list_elem",
    "list_elem_fn",
    "field",
    "field_list_elem",
    "match_payload",
    "loop_var",
    "map_value",
    "cross_module",
];

fn emit_case(ctx: &str, ty: &TypeSpec) -> Option<(String, String)> {
    let (prelude, setup, vexpr) = apply_context(ctx, ty)?;
    let name = format!("matrix_{}_{}", ty.key, ctx);

    let mut expects = Vec::new();
    let mut op_stmts = String::new();
    for (tpl, lines) in ty.ops {
        let stmt = tpl.replace("{V}", &vexpr);
        op_stmts.push_str("    ");
        op_stmts.push_str(&stmt);
        op_stmts.push('\n');
        expects.extend(lines.iter().map(|s| s.to_string()));
    }

    let body = match ctx {
        // Ops laufen in einer eigenen Funktion mit Typ-Parameter
        "param" => format!(
            "fn useIt(v: {t}) -> Nothing {{\n{ops}}}\n\nfn main() -> Int32 {{\n    useIt({lit});\n    return 0;\n}}",
            t = ty.tnx,
            ops = op_stmts,
            lit = ty.lit
        ),
        // Ops laufen im Match-Arm
        "match_payload" => format!(
            "fn main() -> Int32 {{\n    {setup}\n    match b {{\n        Val(v) => {{\n{ops}        }}\n        _ => println(\"none\");\n    }}\n    return 0;\n}}",
            setup = setup,
            ops = op_stmts
                .lines()
                .map(|l| format!("        {l}\n"))
                .collect::<String>(),
        ),
        // Ops laufen im Schleifenkörper
        "loop_var" => format!(
            "fn main() -> Int32 {{\n    {setup}\n    for v in xs {{\n{ops}    }}\n    return 0;\n}}",
            setup = setup,
            ops = op_stmts
                .lines()
                .map(|l| format!("    {l}\n"))
                .collect::<String>(),
        ),
        _ => {
            let setup_line = if setup.is_empty() {
                String::new()
            } else {
                format!("    {setup}\n")
            };
            format!("fn main() -> Int32 {{\n{setup_line}{op_stmts}    return 0;\n}}")
        }
    };

    let mut src = String::new();
    for e in &expects {
        src.push_str(&format!("// expect: {e}\n"));
    }
    src.push('\n');
    src.push_str(&prelude);
    if !prelude.is_empty() {
        src.push('\n');
    }
    src.push_str(&body);
    src.push('\n');
    Some((name, src))
}

fn helper_module() -> String {
    let mut s = String::from("module matrixmod;\n\n");
    for ty in TYPES {
        s.push_str(&format!(
            "fn mk_{key}() -> {t} {{\n    return {lit};\n}}\n\n",
            key = ty.key,
            t = ty.tnx,
            lit = ty.lit
        ));
    }
    s
}

fn generate_all(shard: usize) -> PathBuf {
    // Pro Shard ein eigenes Verzeichnis — die Shards laufen als parallele
    // Threads, ein gemeinsames Verzeichnis würde remove/rewrite-Races geben.
    let dir = std::env::temp_dir().join(format!("tinox-matrix-{}-{shard}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir matrix dir");
    fs::write(dir.join("_matrix_mod.tnx"), helper_module()).expect("write helper");
    for ty in TYPES {
        for ctx in CONTEXTS {
            if let Some((name, src)) = emit_case(ctx, ty) {
                fs::write(dir.join(format!("{name}.tnx")), src).expect("write case");
            }
        }
    }
    dir
}

fn run_shard(shard: usize, num_shards: usize) {
    let dir = generate_all(shard);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "tnx").unwrap_or(false))
        .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    names.sort();

    let mut unexpected_failures = Vec::new();
    let mut stale_entries = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if i % num_shards != shard {
            continue;
        }
        let case = parse_case(&dir.join(format!("{name}.tnx")));
        let known_bad = KNOWN_FAILURES.contains(&name.as_str());
        match run_case(&case) {
            Ok(()) if known_bad => stale_entries.push(name.clone()),
            Ok(()) => {}
            Err(_) if known_bad => {}
            Err(msg) => unexpected_failures.push(format!("== {name} ==\n{msg}")),
        }
    }

    let mut problems = Vec::new();
    if !unexpected_failures.is_empty() {
        problems.push(format!(
            "{} Matrix-Fälle schlagen fehl (neue Inferenz-Löcher — fixen oder in KNOWN_FAILURES eintragen):\n\n{}",
            unexpected_failures.len(),
            unexpected_failures.join("\n\n")
        ));
    }
    if !stale_entries.is_empty() {
        problems.push(format!(
            "stale KNOWN_FAILURES (bestehen inzwischen — Eintrag entfernen): {}",
            stale_entries.join(", ")
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn matrix_shard_0() { run_shard(0, 4); }
#[test]
fn matrix_shard_1() { run_shard(1, 4); }
#[test]
fn matrix_shard_2() { run_shard(2, 4); }
#[test]
fn matrix_shard_3() { run_shard(3, 4); }
