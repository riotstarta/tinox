//! Stdlib-Smoke-Gate: jedes Modul in crates/tinox-core einmal benutzen.
//!
//! Anlass (2026-07-17): mehrere Stdlib-Module riefen Builtins auf, die weder
//! in der Runtime noch im Codegen existieren (httpGet, base64Decode,
//! uuidGenerate, …) — sie kompilierten nie, weil kein Test sie je importiert
//! hat. Da ein Import das ganze Modul codegen't, reicht ein minimaler
//! Aufruf pro Modul, um Ghost-Builtins und Codegen-Brüche im gesamten
//! Modul zu fangen (der IR-Verifier meldet jede unreferenzierte Deklaration).
//!
//! Die .tnx-Fälle werden zur Laufzeit generiert (nichts eingecheckt).
//! Bekannte Brüche stehen in KNOWN_BROKEN: ein gelistetes Modul MUSS
//! fehlschlagen (sonst „stale entry" → Liste pflegen), ein nicht gelistetes
//! MUSS bestehen. Ein Vollständigkeits-Test erzwingt, dass jedes neue
//! Stdlib-Modul hier einen Smoke-Fall bekommt (oder begründet in EXCLUDED
//! steht).

mod common;
use common::{parse_case, run_case};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Module, die heute nicht kompilieren/laufen — jeder Eintrag ist ein
/// offener Bug (siehe bugs.md Bug 20, dort nach Fehlerklasse gruppiert).
/// Beim Fixen: Eintrag entfernen, sonst schlägt der Test mit "stale entry"
/// fehl.
///
/// Seit 2026-07-18 leer: alle tinox-core-Module kompilieren und laufen
/// (Bug 20 vollständig abgearbeitet, Bugs 21-32). Neu kaputte Module hier
/// mit Begründung + bugs.md-Verweis eintragen.
const KNOWN_BROKEN: &[&str] = &[];

/// Module ohne Smoke-Fall, mit Begründung.
const EXCLUDED: &[(&str, &str)] = &[
    ("db", "braucht [database]-Config; von den orm_sqlite_* E2E-Fällen abgedeckt"),
];

struct Smoke {
    /// Dateistamm in crates/tinox-core (= Testname stdlib_smoke_<key>)
    key: &'static str,
    /// Import-Zeilen (meist nur das Modul selbst)
    imports: &'static [&'static str],
    /// Statements im main-Rumpf
    body: &'static str,
    /// Erwartete stdout-Zeilen (exakt, geordnet)
    expects: &'static [&'static str],
    /// Alternativ/zusätzlich: Substrings, die die Ausgabe enthalten muss
    contains: &'static [&'static str],
}

const SMOKES: &[Smoke] = &[
    Smoke {
        key: "array",
        imports: &["tinox.core.array"],
        body: "println(Arrays::findMin([3, 1, 2]));",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "asm",
        imports: &["tinox.core.asm"],
        body: "let a: Assembler = Assembler::new();\n    Assembler::emit(a, 144);\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "base64",
        imports: &["tinox.core.base64"],
        body: r#"println(Base64::encode("hi"));"#,
        expects: &["aGk="],
        contains: &[],
    },
    Smoke {
        key: "bitmap",
        imports: &["tinox.core.bitmap"],
        body: "let b: Bitmap = Bitmap::create(2, 2, 0);\n    Bitmap::setPixel(b, 0, 0, 5);\n    println(Bitmap::getPixel(b, 0, 0));",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "cache",
        imports: &["tinox.core.cache"],
        body: "let c: Cache<String, Int64> = Cache::new(4);\n    Cache::set(c, \"a\", 1);\n    let o: Option<Int64> = Cache::get(c, \"a\");\n    println(o.unwrap());",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "collections",
        imports: &["tinox.core.collections"],
        body: "let s: Stack<Int64> = Stack::new();\n    s.push(7);\n    println(s.pop());",
        expects: &["7"],
        contains: &[],
    },
    Smoke {
        key: "complex",
        imports: &["tinox.core.complex"],
        body: "println(Complex::magnitude(Complex::new(3.0, 4.0)).toString());",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "cron",
        imports: &["tinox.core.cron"],
        body: "let e: CronExpr = Cron::parse(\"* * * * *\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "crypto",
        imports: &["tinox.core.crypto"],
        body: r#"if Crypto::sha256("abc").len() > 0 { println("yes"); } else { println("no"); }"#,
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "csv",
        imports: &["tinox.core.csv"],
        body: "let rows: List<List<String>> = Csv::parse(\"a,b\\nc,d\");\n    println(rows.len());\n    println(rows[0][1]);",
        expects: &["2", "b"],
        contains: &[],
    },
    Smoke {
        key: "debug",
        imports: &["tinox.core.debug"],
        body: "Debug::assert(true, \"never\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "decimal",
        imports: &["tinox.core.decimal"],
        body: "println(Decimal::toString(Decimal::fromInt(3)));",
        expects: &["3"],
        contains: &[],
    },
    Smoke {
        key: "encoding",
        imports: &["tinox.core.encoding"],
        body: "println(Encoding::fromCharCode(65));",
        expects: &["A"],
        contains: &[],
    },
    Smoke {
        key: "env",
        imports: &["tinox.core.env"],
        body: "Env::setVar(\"TINOX_SMOKE\", \"x1\");\n    println(Env::getVar(\"TINOX_SMOKE\"));",
        expects: &["x1"],
        contains: &[],
    },
    Smoke {
        key: "events",
        imports: &["tinox.core.events", "tinox.core.json"],
        body: "let em: EventEmitter = EventEmitter::new();\n    EventEmitter::on(em, \"ping\", v => { println(\"pong\"); });\n    EventEmitter::emit(em, \"ping\", Json::parse(\"1\"));",
        expects: &["pong"],
        contains: &[],
    },
    Smoke {
        key: "fmt",
        imports: &["tinox.core.fmt"],
        body: "println(Fmt::sprintf(\"a%sb\", [\"X\"]));",
        expects: &["aXb"],
        contains: &[],
    },
    Smoke {
        key: "format",
        imports: &["tinox.core.format"],
        body: "println(Format::padLeft(\"7\", 3, \"0\"));",
        expects: &["007"],
        contains: &[],
    },
    Smoke {
        key: "fs",
        imports: &["tinox.core.fs"],
        body: "Fs::writeFile(\"smoke.txt\", \"hi\");\n    println(Fs::readFile(\"smoke.txt\"));",
        expects: &["hi"],
        contains: &[],
    },
    Smoke {
        key: "graph",
        imports: &["tinox.core.graph"],
        body: "let g: Graph<Int64> = Graph::new();\n    Graph::addNode(g, \"a\", 1);\n    Graph::addNode(g, \"b\", 2);\n    Graph::addEdge(g, \"a\", \"b\", 1.0);\n    println(Graph::neighbors(g, \"a\").len());",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "hash",
        imports: &["tinox.core.hash"],
        body: r#"if Hash::hashString("a") == Hash::hashString("a") { println("yes"); } else { println("no"); }"#,
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "heap",
        imports: &["tinox.core.heap"],
        body: "let h: Heap<Int64> = Heap::new();\n    Heap::push(h, 3);\n    Heap::push(h, 1);\n    println(Heap::pop(h));",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "hex",
        imports: &["tinox.core.hex"],
        body: r#"println(Hex::encode("A"));"#,
        expects: &["41"],
        contains: &[],
    },
    Smoke {
        key: "hpack",
        imports: &["tinox.core.hpack"],
        body: "let h: HpackHeader = HpackHeader::new(\"a\", \"b\");\n    println(h.name);",
        expects: &["a"],
        contains: &[],
    },
    Smoke {
        key: "http",
        imports: &["tinox.core.http"],
        body: "Http::setHeader(\"x-a\", \"b\");\n    Http::clearHeaders();\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "http2_server",
        imports: &["tinox.core.http2_server"],
        body: "println(Http2FrameType::HEADERS());",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "http_server",
        imports: &["tinox.core.http_server"],
        body: "let s: HttpServer = HttpServer::new(0);\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "ini",
        imports: &["tinox.core.ini"],
        body: "let c: IniConfig = Ini::parse(\"[s]\\nk=v\");\n    println(IniConfig::getString(c, \"s\", \"k\", \"?\"));",
        expects: &["v"],
        contains: &[],
    },
    Smoke {
        key: "io",
        imports: &["tinox.core.io"],
        body: r#"println(Paths::join("a", "b.txt"));"#,
        expects: &["a/b.txt"],
        contains: &[],
    },
    Smoke {
        key: "iter",
        imports: &["tinox.core.iter"],
        body: "let xs: List<Int64> = Iter::repeat(7, 3);\n    println(xs.len());",
        expects: &["3"],
        contains: &[],
    },
    Smoke {
        key: "json",
        imports: &["tinox.core.json"],
        body: "let v: JsonValue = Json::parse(\"{\\\"a\\\": 5}\");\n    println(Json::getField(v, \"a\").getInt());",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "jwt",
        imports: &["tinox.core.jwt", "tinox.core.json"],
        body: "var p: Map<String, JsonValue> = Map::new();\n    let t: String = Jwt::encode(p, \"secret\");\n    if t.len() > 0 { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "logger",
        imports: &["tinox.core.logger"],
        body: "let l: Logger = Logger::new(\"t\");\n    Logger::info(l, \"hello-smoke\");",
        expects: &[],
        contains: &["hello-smoke"],
    },
    Smoke {
        key: "mathf",
        imports: &["tinox.core.mathf"],
        body: "println(Mathf::sqrt(9.0).toString());",
        expects: &["3"],
        contains: &[],
    },
    Smoke {
        key: "math",
        imports: &["tinox.core.math"],
        body: "println(Math::abs(-5));",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "mathx",
        imports: &["tinox.core.mathx"],
        body: "println(Mathx::gcd(12, 8));",
        expects: &["4"],
        contains: &[],
    },
    Smoke {
        key: "metrics",
        imports: &["tinox.core.metrics"],
        body: "MetricsRegistry::incCounter(\"smoke\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "oauth2",
        imports: &["tinox.core.oauth2"],
        body: "let c: OAuth2Client = OAuth2Client::new(\"https://example.com/authorize\", \"https://example.com/token\", \"cid\", \"csecret\", \"https://app.example.com/cb\");\n    let r: OAuth2AuthorizeRequest = c.buildAuthorizeUrl(\"openid\");\n    if r.url.contains(\"code_challenge=\") { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "option",
        imports: &["tinox.core.option"],
        body: "let o: Option<Int64> = Option::some(5);\n    println(o.unwrap());",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "pool",
        imports: &["tinox.core.pool"],
        // Exercises the factory-callback path (fnc field on a generic class):
        // acquire() calls pool.factory() when nothing is pooled.
        body: "let p: Pool<Int64> = Pool::newWithFactory(2, fnc() -> Int64 { return 7; });\n    println(Pool::acquire(p).toString());",
        expects: &["7"],
        contains: &[],
    },
    Smoke {
        key: "process",
        imports: &["tinox.core.process"],
        body: "if Process::pid() > 0 { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "queue",
        imports: &["tinox.core.queue"],
        body: "let q: PriorityQueue<String> = PriorityQueue::new();\n    PriorityQueue::enqueue(q, \"a\", 1);\n    println(PriorityQueue::dequeue(q));",
        expects: &["a"],
        contains: &[],
    },
    Smoke {
        key: "random",
        imports: &["tinox.core.random"],
        body: "let x: Int64 = Random::nextInt(6);\n    if x >= 0 { println(\"ok\"); } else { println(\"ok\"); }",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "ratelimit",
        imports: &["tinox.core.ratelimit"],
        body: "let r: RateLimiter = RateLimiter::new(2, 1000);\n    if RateLimiter::allow(r) { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "regex",
        imports: &["tinox.core.regex"],
        body: r#"if Regex::isMatch("ab+", "abb") { println("yes"); } else { println("no"); }"#,
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "rest.server",
        imports: &["tinox.core.rest.server"],
        body: "let g: GET = GET::new(\"/x\");\n    println(g.path);",
        expects: &["/x"],
        contains: &[],
    },
    Smoke {
        key: "rest.client",
        imports: &["tinox.core.rest.client"],
        body: "let c: RestClient = RestClient::new(\"http://localhost:1\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "result",
        imports: &["tinox.core.result"],
        body: "let r: Result<Int64> = Result::ok(5);\n    println(r.unwrap());",
        expects: &["5"],
        contains: &[],
    },
    Smoke {
        key: "semaphore",
        imports: &["tinox.core.semaphore"],
        body: "let s: Semaphore = Semaphore::new(1);\n    if Semaphore::tryAcquire(s) { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "set",
        imports: &["tinox.core.set"],
        body: "let s: Set<Int64> = Set::new();\n    Set::add(s, 1);\n    Set::add(s, 1);\n    println(Set::size(s));",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "socket",
        imports: &["tinox.core.socket"],
        body: "let s: Socket = Socket::createTcp();\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "sort",
        imports: &["tinox.core.sort"],
        body: "let xs: List<Int64> = Sort::quickSort([3, 1, 2]);\n    println(xs[0]);",
        expects: &["1"],
        contains: &[],
    },
    Smoke {
        key: "string",
        imports: &["tinox.core.string"],
        body: r#"println(Strings::toUpperCase("ab"));"#,
        expects: &["AB"],
        contains: &[],
    },
    Smoke {
        key: "time",
        imports: &["tinox.core.time"],
        body: "let d: Duration = Time::newDuration(120);\n    println(d.getMinutes());",
        expects: &["2"],
        contains: &[],
    },
    Smoke {
        key: "toml",
        imports: &["tinox.core.toml"],
        body: "let v: TomlValue = Toml::parse(\"a = 1\");\n    if v.isTable() { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "tpl",
        imports: &["tinox.core.tpl", "tinox.core.json"],
        body: "var m: Map<String, JsonValue> = Map::new();\n    println(Template::render(\"hi\", m));",
        expects: &["hi"],
        contains: &[],
    },
    Smoke {
        key: "trie",
        imports: &["tinox.core.trie"],
        body: "let t: Trie = Trie::new();\n    Trie::insert(t, \"ab\");\n    if Trie::search(t, \"ab\") { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "uri",
        imports: &["tinox.core.uri"],
        body: r#"println(Uri::encode("a b"));"#,
        expects: &["a%20b"],
        contains: &[],
    },
    Smoke {
        key: "uuid",
        imports: &["tinox.core.uuid"],
        body: "println(Uuid::generate().len());",
        expects: &["36"],
        contains: &[],
    },
    Smoke {
        key: "amqp091",
        imports: &["tinox.core.amqp091"],
        // Kein echter Broker im Smoke-Gate — 127.0.0.1 auf einem Port ohne
        // Listener liefert ein schnelles "connection refused" (kein DNS,
        // keine Latenz), deckt aber den kompletten Codegen-Pfad des Moduls
        // ab (dial -> socketCreateTcp/socketConnect/httpConnFromFd).
        body: "println(Amqp091::dial(\"127.0.0.1\", 39217));",
        expects: &["-1"],
        contains: &[],
    },
    Smoke {
        key: "amqp10",
        imports: &["tinox.core.amqp10"],
        // Analog zu amqp091 oben: kein echter Broker im Smoke-Gate, deckt
        // aber den Codegen-Pfad des Moduls ab.
        body: "println(Amqp10::dial(\"127.0.0.1\", 39220));",
        expects: &["-1"],
        contains: &[],
    },
    Smoke {
        key: "websocket",
        imports: &["tinox.core.websocket"],
        // Reine Codec-Logik ohne Socket (die volle Strecke inkl. Handshake
        // deckt tests/e2e/ws_handshake_frames.tnx ab).
        body: "let f: WsFrame = WsFrame { fin: true, opcode: 1, payload: Ws::textToBytes(\"hi\") };\n    println(Ws::text(f));",
        expects: &["hi"],
        contains: &[],
    },
    Smoke {
        key: "validation",
        imports: &["tinox.core.validation"],
        body: r#"if Validation::isNumeric("123") { println("yes"); } else { println("no"); }"#,
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "xml",
        imports: &["tinox.core.xml"],
        body: "let n: XmlNode = Xml::parse(\"<a>x</a>\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
    Smoke {
        key: "yaml",
        imports: &["tinox.core.yaml"],
        body: "let v: YamlValue = Yaml::parse(\"a: 1\");\n    if v.isMap() { println(\"yes\"); } else { println(\"no\"); }",
        expects: &["yes"],
        contains: &[],
    },
    Smoke {
        key: "zip",
        imports: &["tinox.core.zip"],
        body: "let a: ZipArchive = Zip::create(\"smoke.zip\");\n    println(\"ok\");",
        expects: &["ok"],
        contains: &[],
    },
];

fn stdlib_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tinox-core")
}

fn emit_case(s: &Smoke) -> (String, String) {
    let name = format!("stdlib_smoke_{}", s.key);
    let mut src = String::new();
    for e in s.expects {
        src.push_str(&format!("// expect: {e}\n"));
    }
    for c in s.contains {
        src.push_str(&format!("// expect-contains: {c}\n"));
    }
    src.push('\n');
    for imp in s.imports {
        src.push_str(&format!("import {imp};\n"));
    }
    src.push_str(&format!(
        "\nfn main() -> Int32 {{\n    {}\n    return 0;\n}}\n",
        s.body
    ));
    (name, src)
}

/// Jedes Stdlib-Modul hat einen Smoke-Fall oder steht begründet in EXCLUDED.
#[test]
fn stdlib_smoke_completeness() {
    // A module is either a legacy single `<name>.tnx` file (not yet migrated),
    // a `<name>/` directory of one-type-per-file `.tnx` files directly inside
    // it (migrated, one-type-per-file convention), or — since the compiler
    // gained nested `tinox.core.X.Y` stdlib import support (rest/client,
    // rest/server) — a `<name>/` directory containing ONLY subdirectories
    // (no `.tnx` files of its own), a pure grouping directory: descend one
    // level and key each child as `"<name>.<child>"`, matching the import
    // path (`tinox.core.rest.client`) rather than the directory name.
    let modules: BTreeSet<String> = fs::read_dir(stdlib_dir())
        .expect("crates/tinox-core lesbar")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .flat_map(|p| -> Vec<String> {
            if p.is_dir() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string());
                let Some(name) = name else { return vec![] };
                let has_own_tnx = fs::read_dir(&p)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .any(|e| e.path().extension().map(|x| x == "tnx").unwrap_or(false))
                    })
                    .unwrap_or(true); // unreadable -> don't misclassify as a grouping dir
                if has_own_tnx {
                    vec![name]
                } else {
                    fs::read_dir(&p)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .filter(|e| e.path().is_dir())
                                .filter_map(|e| e.file_name().to_str().map(|s| format!("{name}.{s}")))
                                .collect()
                        })
                        .unwrap_or_default()
                }
            } else if p.extension().map(|x| x == "tnx").unwrap_or(false) {
                p.file_stem()
                    .map(|s| vec![s.to_string_lossy().to_string()])
                    .unwrap_or_default()
            } else {
                vec![]
            }
        })
        .collect();
    let covered: BTreeSet<String> = SMOKES.iter().map(|s| s.key.to_string()).collect();
    let excluded: BTreeSet<String> = EXCLUDED.iter().map(|(k, _)| k.to_string()).collect();

    let missing: Vec<&String> = modules
        .iter()
        .filter(|m| !covered.contains(*m) && !excluded.contains(*m))
        .collect();
    assert!(
        missing.is_empty(),
        "Stdlib-Module ohne Smoke-Fall (Eintrag in SMOKES ergänzen oder mit Begründung in EXCLUDED): {missing:?}"
    );

    let stale: Vec<&String> = covered
        .iter()
        .chain(excluded.iter())
        .filter(|k| !modules.contains(*k))
        .collect();
    assert!(
        stale.is_empty(),
        "SMOKES/EXCLUDED-Einträge ohne Modul-Datei (gelöschtes Modul? Eintrag entfernen): {stale:?}"
    );

    for k in KNOWN_BROKEN {
        assert!(
            covered.contains(*k),
            "KNOWN_BROKEN-Eintrag {k:?} hat keinen Smoke-Fall"
        );
    }
}

fn generate_all(shard: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tinox-stdlib-smoke-{}-{shard}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir smoke dir");
    for s in SMOKES {
        let (name, src) = emit_case(s);
        fs::write(dir.join(format!("{name}.tnx")), src).expect("write case");
    }
    dir
}

fn run_shard(shard: usize, num_shards: usize) {
    let dir = generate_all(shard);
    let mut unexpected_failures = Vec::new();
    let mut stale_entries = Vec::new();
    for (i, s) in SMOKES.iter().enumerate() {
        if i % num_shards != shard {
            continue;
        }
        let name = format!("stdlib_smoke_{}", s.key);
        let case = parse_case(&dir.join(format!("{name}.tnx")));
        let known_bad = KNOWN_BROKEN.contains(&s.key);
        match run_case(&case) {
            Ok(()) if known_bad => stale_entries.push(s.key.to_string()),
            Ok(()) => {}
            Err(_) if known_bad => {}
            Err(msg) => unexpected_failures.push(format!("== {name} ==\n{msg}")),
        }
    }

    let mut problems = Vec::new();
    if !unexpected_failures.is_empty() {
        problems.push(format!(
            "{} Stdlib-Smoke-Fälle schlagen fehl (Modul kaputt → fixen oder in KNOWN_BROKEN + bugs.md eintragen):\n\n{}",
            unexpected_failures.len(),
            unexpected_failures.join("\n\n")
        ));
    }
    if !stale_entries.is_empty() {
        problems.push(format!(
            "stale KNOWN_BROKEN (bestehen inzwischen — Eintrag entfernen): {}",
            stale_entries.join(", ")
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn stdlib_smoke_shard_0() { run_shard(0, 4); }
#[test]
fn stdlib_smoke_shard_1() { run_shard(1, 4); }
#[test]
fn stdlib_smoke_shard_2() { run_shard(2, 4); }
#[test]
fn stdlib_smoke_shard_3() { run_shard(3, 4); }
