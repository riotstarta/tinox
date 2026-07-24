# Tinox — AMQP-Client-Roadmap (tasklist.md)

Stand: 2026-07-22. Zweck: Statusübersicht + Wiedereinstiegspunkt über Sessions
hinweg. **Beim Weiterarbeiten: den nächsten offenen Task nehmen (erster `[ ]`
von oben), nach Abschluss hier abhaken und ggf. Notizen ergänzen.**

Status: `[ ]` offen · `[~]` in Arbeit · `[x]` fertig · `[-]` bewusst gestrichen

Arbeitsregeln (Projektkonvention): nach jedem Schritt `make check` komplett
grün; Features/Bugs in bugs.md dokumentieren (Stil: Status/Ursache/Verifiziert);
e2e-Tests unter `tests/e2e/*.tnx` mit `// expect:`-Direktiven; kein
Silent-Garbage — im Zweifel harter Fehler.

Vorheriges Feature (WebSocket-Server + `@WebsocketEndpoint`-Annotationen) ist
komplett fertig und in `bugs.md` archiviert (Suche „Feature: WebSocket").
Diese Datei startet frisch für AMQP.

---

## Ziel

AMQP-**Client** als Sprachfeature/Stdlib-Modul — verbindet sich zu einem
bestehenden Broker (RabbitMQ, ActiveMQ Artemis, Qpid, Azure Service Bus, …),
sendet und empfängt Nachrichten. **Kein Broker/Server** (Entscheidung Session
2026-07-22: Scope wäre um ein Vielfaches größer — Exchange-Routing-Engine,
Queue-Persistenz, Consumer-Flow-Control, Clustering; ein Client, der sich zu
existierender Infrastruktur verbindet, ist der weit tragfähigere Umfang).

Zwei Protokollversionen, **sequenziell, 0-9-1 zuerst**:

- **AMQP 0-9-1** (Phasen 1–5, dieser Roadmap-Abschnitt): das von RabbitMQ
  dominierte Protokoll — Classes/Methods, Field-Tables, einfacheres
  Frame-Format. Leichter live gegen einen echten Broker verifizierbar, gutes
  Fundament für die Conn-Handle-Wiederverwendung.
- **AMQP 1.0** (eigene Roadmap-Phase, s. „Später" — wird erst grob skizziert,
  detailliert geplant sobald 0-9-1 steht): ISO/IEC-19464-Standard,
  komplett anderes Typsystem (Performatives statt Classes/Methods,
  beschriebene Composite-Types), u. a. bei Qpid/Artemis/Azure Service Bus.
  **Kein gemeinsamer Wire-Code mit 0-9-1** — beide Versionen teilen sich nur
  die Transport-Schicht (TCP-Dial + Conn-Handle), nicht den Frame-Codec.

## Befund (Sondierung 2026-07-22, Session-Kontext)

- Kein AMQP im Repo (Runtime, Stdlib, Docs, Beispiele geprüft — 0 Treffer für
  „amqp"/„rabbitmq").
- **Wiederverwendbar aus der WebSocket-Arbeit** (kompletter Vorteil ggü. dem
  WS-Start): binärsichere Conn-Primitiven `httpConnReadN`/`httpConnWriteBytes`
  existieren schon und sind bereits FÜR CLIENT-SEITIGE Verbindungen erprobt
  (`httpConnFromFd(fd)` wickelt einen nackten Socket-fd — accept- oder
  connect-seitig — in ein Conn-Handle; in `ws_handshake_frames.tnx` schon als
  Client-Rolle genutzt). AMQP-Frames sind wie WS-Frames reines Binärformat
  (Längenpräfix + beliebige Bytes) — dieselbe Grundlage trägt.
- `socket.tnx`: `socketCreateTcp()` + `socketConnect(fd, host, port)` zum
  Verbindungsaufbau zum Broker vorhanden.
- **Blocker 1 — kein TLS-Client:** nur `HttpServer::listenTls` (Server-Accept)
  existiert; kein `connectTls`/Client-Handshake. Blockiert `amqps://`
  (Port 5671) UND AMQP-1.0-über-TLS. v1 = Klartext `amqp://` (Port 5672) für
  beide Protokollversionen; TLS-Client als eigener Folgetask (s. „Später" —
  analog zur WS-wss-Lücke, aber hier härter: AMQP wird in der Praxis oft nur
  mit TLS betrieben, das ist eine echte Einschränkung für produktiven Einsatz).
- **Blocker 2 — Field-Value-Encoder (0-9-1):** Methodenargumente sind
  typmarkiert (bit/octet/short/long/longlong/shortstr/longstr/table/
  timestamp/decimal/boolean …), verschachtelt in Field-Tables (z. B.
  `client-properties` in `connection.start-ok`). Kein Präzedenzfall im Repo
  für GENAU dieses Format, aber `hpack.tnx` zeigt, dass ein Binärcodec dieser
  Art rein in Tinox über den Byte-Primitiven machbar ist.
- **Blocker 3 — Kanal-Multiplexing:** eine TCP-Verbindung trägt mehrere
  „Channels" (leichtgewichtige virtuelle Verbindungen, jede Frame trägt eine
  Channel-ID). v1 nutzt einen einzelnen Channel synchron (kein Multiplexing
  mehrerer gleichzeitig offener Channels) — analog zur v1-Entscheidung bei WS
  gegen Lambda-Handler: erst das Fundament, Nebenläufigkeit später.
- **Live-Verifikation:** kein Broker lokal aktiv, aber Docker vorhanden
  (`docker run -d --name tinox-amqp-test -p 5672:5672 -p 15672:15672
  rabbitmq:3-management` — hat 0-9-1 nativ; für spätere 1.0-Tests das
  `rabbitmq_amqp1_0`-Plugin aktivieren oder auf Qpid/Artemis wechseln).
  Kein `pika` (Python-AMQP-Client) installiert — bei Bedarf für Live-Tests
  `pip install pika` (analog zu `python-websockets` bei WS 5.1).

---

## Phase 1 — Transport + Protokoll-Handshake (`crates/tinox-core/amqp091.tnx`) ✅

- [x] 1.1 Modul-Skelett `tinox.core.amqp091`, Namespace `tinox.core.amqp091`
      (importiert selbst `tinox.core.socket` — `socketCreateTcp`/
      `socketConnect` sind Modul-lokale `extern fn` in socket.tnx, keine
      globalen Builtins wie `httpConnFromFd`; ohne eigenen Import bricht
      jeder Konsument, der nicht zufällig auch `socket` importiert, mit
      "undefined value" — im stdlib_smoke-Gate aufgefallen, s. Log).
      `Amqp091::dial(host, port) -> Int64`: `socketCreateTcp` + `socketConnect`
      + `httpConnFromFd` (Conn-Handle, binärsicher). Fehlerpfad: `connect`
      liefert `false` → hartes `-1`, kein Silent-Garbage.
- [x] 1.2 Protokoll-Header-Handshake (AMQP 0-9-1 §4.2.2): `sendProtocolHeader`
      sendet die 8 Bytes `"AMQP" 0x00 0x00 0x09 0x01`; `checkHandshakeResponse`
      liest 1 Byte und akzeptiert NUR Wert 1 (Beginn eines echten
      METHOD-Frames = `connection.start`) — Mismatch (Broker echot seinen
      Header zurück, erstes Byte 'A'=65) und EOF/Kurzread beide harter
      Fehlschlag. Bewusst als ZWEI getrennte Methoden (statt eine
      blockierende Rundtrip-Funktion), damit sich Client/Server-Schritte im
      e2e-Test (single-threaded) verschränken lassen.
- [x] 1.3 e2e `tests/e2e/amqp_phase1_handshake.tnx`: dial-refused (Port ohne
      Listener), Header-Bytes-Assertion, Handshake success/mismatch/EOF —
      alle über echte Loopback-Conn (Server-Rolle mit
      `httpServerAcceptConnHandle` + gescriptete Antwort-Bytes, Muster wie
      `ws_handshake_frames.tnx`). Dazu stdlib_smoke-Eintrag `amqp091`
      (dial gegen 127.0.0.1 auf unbelegtem Port → schnelles "-1", deckt den
      Codegen-Pfad ohne echten Broker ab).

## Phase 2 — Frame-Codec + Field-Value-Encoder ✅

- [x] 2.1 Generisches Frame: `octet type | short chanId | long size | payload
      | octet 0xCE (frame-end)`. `Amqp091::readFrame(conn) -> AmqpFrame091`
      (analog `Ws::readFrame`), `writeFrame(conn, type, chanId, payload)`.
      Frame-Typen: 1=METHOD, 2=HEADER, 3=BODY, 4=HEARTBEAT. Fehlender
      0xCE-Terminator ODER Payload > 16-MB-Cap → harter Protokollfehler
      (frameType -2, kein Silent-Garbage); EOF → -1.
      (Feld hieß ursprünglich `channel` — reserviertes Keyword für die
      Concurrency-Feature, umbenannt zu `chanId`, s. Log.)
- [x] 2.2 Field-Value-Codec: `AmqpFieldValue091`-Enum (rekursiv über
      `TableVal(Map<String,AmqpFieldValue091>)`/`ArrayVal(List<...>)` —
      funktioniert, weil Map/List Handle-Typen sind, kein Größenproblem) +
      `AmqpWriter091`/`AmqpReader091` als Byte-Builder/-Cursor. Abgedeckt:
      `t`(bool)/`I`(int32 signed)/`L`(int64 signed)/`S`(longstr)/
      `T`(timestamp)/`F`(nested table)/`A`(array)/`V`(void), RabbitMQ-
      Typmarker-Konvention. Unbekannter Typmarker → `ErrorVal(String)` statt
      Silent-Garbage (Aufrufer MUSS prüfen, analog WsFrame -1/-2).
      **Nicht v1:** `bit`-Packing in Methodenargumenten (gehört zu Phase 3,
      wenn die ersten Methoden mit bit-Feldern gebaut werden), `decimal`.
- [x] 2.3 Generische METHOD-Frame-Hülle (`AmqpMethodFrame091`: class-id +
      method-id + Rest-Bytes) über `encodeMethodPayload`/`decodeMethodPayload`.
      **Scope-Korrektur während der Umsetzung:** die konkrete
      Argument-Reihenfolge pro v1-Methode (`connection.start-ok` etc.) UND
      der Content-Header-Property-Codec (`basic`-Klasse) sind fachlich
      Phase-3/4-Arbeit, nicht Teil des generischen Frame-Codecs — auf die
      jeweilige Phase verschoben.
- [x] 2.4 e2e `tests/e2e/amqp_frame_codec.tnx`: shortstr/longstr-Rundtrip,
      Field-Table mit gemischten Typen (bool/int32 negativ/int64/str/
      timestamp/void), verschachtelte Table, Array, unbekannter Typmarker,
      Methoden-Envelope-Rundtrip, Frame-Rundtrip über echte Loopback-Conn,
      Protokollfehler (falscher Terminator), Payload-Cap, EOF.

**Zwei Bugs beim Aufbau gefunden und gefixt (s. Log):** ein echter
Tinox-Compiler-Bug (Bug 66, `fromCharCode(seiteneffektbehafteter Ausdruck)`
wertete doppelt aus — gefixt in `codegen.rs`, bugs.md-Eintrag) und ein reiner
Logikfehler in `AmqpReader091::long()` (baute 32-Bit-Werte unsigned auf, ein
negativer `Int32Val` wie -42 kam als 4294967254 zurück — Fix: eigene
`longSigned()`-Methode für den Field-Value-Typ `I`, `long()` bleibt unsigned
für Größenfelder).

## Phase 3 — Connection/Channel-Handshake (Verbindungsaufbau) ✅

- [x] 3.1 Negotiation-State-Machine nach Protokoll-Header: `connection.start`
      empfangen (Mechanisms geprüft — muss `PLAIN` enthalten, sonst harter
      Fehler statt eines Requests, den der Broker ohnehin ablehnt;
      Server-Properties gelesen aber v1 nicht ausgewertet) →
      `connection.start-ok` (SASL PLAIN: Response als `List<Int64>` roh
      gebaut — NUL + User + NUL + Pass —, bewusst NICHT als Tinox-String
      wegen des eingebetteten NUL-Bytes; Client-Properties als Field-Table)
      → `connection.tune` empfangen → `connection.tune-ok` (Server-
      Vorschläge übernommen, Heartbeat fest auf 0 = deaktiviert, kein
      Heartbeat-Sender in v1) → `connection.open` (vhost) →
      `connection.open-ok`.
- [x] 3.2 `AmqpConnection091::connect(host, port, vhost, user, pass) ->
      AmqpConnection091` kapselt 1.1–3.1 komplett; `.conn <= 0` +
      `.errorMessage` bei jedem Fehlschlag. `connection.close` vom Broker
      (SASL-Fehler, unbekannter vhost) wird spec-konform mit `close-ok`
      quittiert (`describeAndAckClose`-Helper, geteilt mit Channel-Close)
      und als Fehlermeldung mit Reply-Code/-Text zurückgegeben — kein
      Hängenbleiben. `AmqpConnection091::close()` für den sauberen
      Verbindungsabbau.
- [x] 3.3 `AmqpChannel091::open(connection) -> AmqpChannel091`: `channel.open`
      + `channel.open-ok` auf festem Channel 1 (v1: kein Pool, s. „Später").
- [x] 3.4 **Live gegen echten RabbitMQ-Container verifiziert** (Docker,
      `rabbitmq:3-management`, Host-Port 55672): Erfolgspfad (Connect +
      Channel-Open + Close) — `frameMax=131072`, `channelMax=2047` (RabbitMQ-
      Defaults, korrekt ausgehandelt), Broker-Log bestätigt SASL-Auth +
      sauberen Verbindungsabbau. ZWEI Negativpfade live verifiziert: falsches
      Passwort (Broker schließt den rohen Socket ohne Frame nach ~3 s
      Anti-Bruteforce-Delay → sauber als EOF/-1 erkannt) und unbekannter
      vhost (Broker schickt echtes `connection.close` mit Reply-Code 530,
      Text wortgleich durchgereicht). Zusätzlich automatisierter
      Loopback-e2e `tests/e2e/amqp_connection_negotiation.tnx`: ein per
      `spawn` gestarteter simulierter Broker (`async fn`) spielt den
      kompletten Ablauf gegen den echten Client-Code — kein Docker in
      `make check` nötig.

**Ein neuer Compiler-Bug gefunden beim Bau des Loopback-e2e (Bug 67,
bugs.md, NICHT gefixt):** `async fn ... -> Bool` + `await` verliert den
Rückgabetyp (Typechecker führt das Ergebnis als `Int64`). Workaround im Test:
die simulierte Broker-Funktion gibt `Int64` (0/1) statt `Bool` zurück.

## Phase 4 — Publish/Consume-API (explizite Schleifen-API, v1) ✅

Bewusst wie WS v1 KEIN Lambda-Handler (dieselbe Begründung: Lambdas als Params
von User-Methoden sind noch nicht zuverlässig gedeckt) — explizite
Poll-Schleife für Consumer, Handler-API als Folgetask analog zu
`@WebsocketEndpoint`/`@On*` sobald das Fundament steht.

- [x] 4.1 `AmqpChannel091::declareQueue(name, durable, exclusive, autoDelete)
      -> String` (`queue.declare` + `-ok`). "" = Fehler (echte Queue-Namen
      sind nie leer), `this.errorMessage` beschreibt die Ursache.
- [x] 4.2 `AmqpChannel091::bindQueue(queue, exchange, routingKey) -> Bool`
      (`queue.bind` + `-ok`). **Wichtige Erkenntnis (live gefunden):**
      `queue.bind` auf den Default-Exchange (`""`) ist laut AMQP-Spec
      VERBOTEN — Queues sind dort implizit unter ihrem eigenen Namen
      gebunden, RabbitMQ antwortet mit `access_refused` und schließt den
      Channel. Publish direkt mit Routing-Key = Queue-Name deckt den
      einfachsten Fall ab OHNE `bindQueue`; für benannte Exchanges (v1 ohne
      `exchange.declare`) eignen sich broker-vordefinierte wie
      `amq.direct` zum Testen.
- [x] 4.3 `AmqpChannel091::publish(exchange, routingKey, body: List<Int64>,
      contentType: String) -> Nothing` (`basic.publish` + Content-Header +
      Body-Frame(s), Split bei `frameMax - 8` Byte Overhead-Abzug). v1:
      genau zwei Properties (content-type optional, delivery-mode fest auf
      2/persistent) — voller Property-Satz s. „Später".
- [x] 4.4 `AmqpChannel091::consume(queue) -> String` (`basic.consume` +
      `-ok`, liefert Consumer-Tag oder "" bei Fehler) und
      `AmqpChannel091::nextMessage() -> AmqpMessage091` (blockierender Pull:
      `basic.deliver` + Content-Header + Body-Frame(s) bis `body-size`
      Bytes — Decoder überspringt alle 14 Basic-Properties korrekt anhand
      der Property-Flags, sonst liefe der Byte-Cursor bei fremden
      Nachrichten mit z. B. `headers` aus der Spur). `.ok == false` bei
      jedem Fehler (kein Silent-Garbage). `AmqpChannel091::ack(deliveryTag)
      -> Nothing`.
- [x] 4.5 `AmqpChannel091::qos(prefetchCount) -> Bool` (`basic.qos` + `-ok`).
- [x] 4.6 Beispiel `examples/amqp_publish_consume.tnx`: verbindet,
      deklariert durable Queue, published 3 Nachrichten über den
      Default-Exchange, konsumiert sie in FIFO-Reihenfolge zurück, printet +
      acked jede einzeln — live gegen RabbitMQ verifiziert.

**Live gegen RabbitMQ verifiziert** (voller Ablauf: Connect → Channel →
declareQueue → bindQueue auf `amq.direct` → qos → zwei Publishes über
verschiedene Exchanges → consume → zwei nextMessage()-Aufrufe mit korrektem
Content-Type/Exchange/Routing-Key/Delivery-Tag → ack → close — alles beim
ersten funktionierenden Versuch nach dem Bind-Fix durchgelaufen). Zusätzlich
automatisierter Loopback-e2e
`tests/e2e/amqp_publish_consume_loopback.tnx` (simulierter Broker via
`spawn`, spiegelt den Publish-Body im simulierten `basic.deliver` zurück,
Rundtrip-Assertion auf den Body-Inhalt).

## Phase 5 — Härtung + Abschluss ✅

- [x] 5.1 Live-Test gegen ECHTEN RabbitMQ-Container: Connect, Queue-Deklaration,
      Publish/Consume-Roundtrip, Ack, sauberes `channel.close`/
      `connection.close`. Mit `pika` (Python, in einem venv installiert) als
      unabhängiger Cross-Check — **beide Richtungen** verifiziert: Tinox
      publiziert → pika konsumiert (3 Nachrichten, Content-Type korrekt), UND
      pika publiziert → Tinox konsumiert (3 Nachrichten, Content-Type
      korrekt). Broker-Log bestätigt saubere Verbindungsschlüsse ohne
      Fehler auf beiden Seiten.
- [x] 5.2 Grenzfälle: leere Message-Bodies (0 Body-Frames) UND Bodies über
      mehrere Frames (frame-max künstlich auf 100 gesetzt, 250-Byte-Body ->
      3 Frames) in BEIDE Richtungen (`publish()` UND `nextMessage()`) —
      `tests/e2e/amqp_edge_cases.tnx`, 25× wiederholt lauffähig (Flakiness
      ausgeschlossen). `shortstr`-Längengrenze (255 Byte) —
      `tests/e2e/amqp_shortstr_limits.tnx`, netzwerkfrei/deterministisch,
      255-Byte-Grenzfall zusätzlich live gegen RabbitMQ verifiziert. SASL-
      Fehler (falsches Passwort) bereits in Phase 3.4 live verifiziert
      (sauberer Fehler statt Hänger) — hier nicht erneut wiederholt.
      **Echter Bug gefunden + gefixt:** `AmqpWriter091::shortstr()` schrieb
      die Länge unmaskiert per `octet(s.len())` — bei > 255 Byte erzeugte
      das `& 0xFF` in `octet()` einen zu KLEINEN Längen-Präfix, während die
      vollen Rohbytes trotzdem folgten (korrupter Frame statt hartem
      Fehler, Verstoß gegen „kein Silent-Garbage"). Fix: `tooLong`-Flag +
      `hasError()`, geprüft in `declareQueue`/`bindQueue`/`consume`/
      `publish` VOR dem Senden. **Zweiter Fund (Bug 68, bugs.md, NICHT
      gefixt):** beim Testen dieser Grenze legte eine Kombination aus
      langem (150+ Byte) Queue-Namen + einem clientseitig verworfenen
      Aufruf im selben `spawn`/`await`-Loopback-Test einen
      nichtdeterministischen Absturz in der Async-Runtime frei — reproduziert
      auch ganz ohne AMQP-Semantik, nicht reproduzierbar außerhalb von
      `spawn`/`await` (live gegen RabbitMQ fehlerfrei). Praktisch
      unerreichbar im v1-Client (überlange Namen werden immer clientseitig
      ohne Netzwerkaufruf verworfen) — dokumentiert, nicht blockierend, die
      ausgelieferten Tests vermeiden das Muster bewusst.
      Nicht separat getestet: verschachtelte Field-Tables über 2 Ebenen
      hinaus (2-Ebenen-Rundtrip bereits in Phase 2 `amqp_frame_codec.tnx`
      abgedeckt, der Codec ist generisch-rekursiv, kein Grund zur Annahme
      einer Tiefenbeschränkung).
- [x] 5.3 bugs.md-Abschnitt „Feature: AMQP-0-9-1-Client" (Architektur +
      bewusste Lücken, Stil wie beim WS-Feature, inkl. Bug-68-Verweis).
      README.md: neuer Abschnitt „AMQP-0-9-1 Client" + Feature-Tabellen-
      Zeile + Modul-Tabellen-Zeile („Messaging"-Kategorie). docs.html: neue
      `#mod-amqp091`-Sektion (Klassen/Methoden-Tabelle + Beispiel + v1-
      Lücken) + Sidebar-Link + Card-Grid-Eintrag in der Stdlib-Übersicht.

---

## Später
- [ ] **AMQP 1.0** — eigene Roadmap-Phase (Grobskizze, wird erst detailliert
      geplant sobald 0-9-1 steht):
  - Typsystem-Codec (Primitives mit variabler Breite, described types,
    composite types via Descriptor + List-Encoding) — deutlich größer als
    der 0-9-1-Field-Value-Codec, eigene Phase wert.
  - Frame/Performative-Codec (`open`, `begin`, `attach`, `transfer`, `flow`,
    `disposition`, `detach`, `end`, `close` statt Classes/Methods).
  - SASL-Negotiation (eigenes Framing, `SASL-init`/`outcome` vor dem
    eigentlichen AMQP-Handshake — anders als der 0-9-1-Weg über
    `connection.start-ok`).
  - Connection/Session/Link-State-Machine (Sessions und Links sind in 1.0
    explizite, langlebige Objekte — kein Äquivalent in 0-9-1).
  - Transfer/Flow/Disposition-Nachrichtenfluss für Publish/Consume.
  - **Kein gemeinsamer Code mit `amqp091.tnx`** außer der Transport-Schicht
    (Conn-Handle-Dial) — eigenes Modul `amqp10.tnx`.
- [ ] TLS-Client (`amqps://`, Port 5671) — braucht eine neue
      Runtime-Primitive (`socketConnectTls`/Client-Handshake), existiert noch
      nirgends im Repo (bisher nur Server-TLS via `listenTls`). Blockiert
      BEIDE Protokollversionen für produktiven Einsatz.
- [ ] Publisher-Confirms (`confirm.select` + `basic.ack`/`basic.nack` vom
      Broker), Transaktionen (`tx.select`/`tx.commit`/`tx.rollback`).
- [ ] Mehrere Channels gleichzeitig (Multiplexing, Nebenläufigkeit) statt des
      v1-Einzel-Channels.
- [ ] `exchange.declare` (benannte/typisierte Exchanges: direct/fanout/
      topic/headers) — v1 nutzt nur den Default-Exchange.
- [ ] Annotation-getriebene Consumer-API (`@Consumer`/`@OnMessage`-Äquivalent,
      analog zu `@WebsocketEndpoint`) — Folgetask sobald die
      Lambda-als-Methodenparam-Lücke geschlossen ist (s. Bugs.md).
- [ ] Heartbeat-getriebene Connection-Recovery (Auto-Reconnect bei
      Verbindungsabbruch).
- [ ] Consumer-Flow-Control jenseits von `basic.qos` (z. B. `basic.cancel`,
      dynamisches Prefetch-Tuning).

---

## Log

- 2026-07-22: Roadmap angelegt (Sondierungsbefund aus Session; noch kein
  Code). Scope-Entscheidungen: Client (kein Broker), 0-9-1 vor 1.0,
  sequenziell statt parallel.
- 2026-07-22: Phase 1 komplett (amqp091.tnx: dial/sendProtocolHeader/
  checkHandshakeResponse; e2e amqp_phase1_handshake.tnx + stdlib_smoke-
  Eintrag). Stolperstein: `socketCreateTcp`/`socketConnect` sind
  Modul-lokale `extern fn` in socket.tnx (anders als die globalen
  httpConn*-Builtins) — amqp091.tnx muss `tinox.core.socket` selbst
  importieren, sonst bricht jeder Konsument ohne eigenen Socket-Import mit
  "undefined value" (im stdlib_smoke-Gate aufgefallen, dort nur amqp091
  importiert — der e2e-Test hatte es durch einen redundanten eigenen
  Socket-Import verdeckt). make check grün. Nächster Schritt: Phase 2
  (Frame-Codec + Field-Value-Encoder).
- 2026-07-22: Phase 2 komplett (Frame-Codec, AmqpFieldValue091-Enum,
  AmqpWriter091/AmqpReader091, METHOD-Frame-Hülle; e2e amqp_frame_codec.tnx).
  Zwei Bugs gefunden+gefixt: (1) `channel` als Feldname kollidiert mit dem
  Concurrency-Keyword — umbenannt zu `chanId`. (2) ECHTER Compiler-Bug
  (Bug 66, bugs.md): `fromCharCode(this.octet())` wertete das
  seiteneffektbehaftete Argument doppelt aus, weil der `"fromCharCode"`-Arm
  in `codegen.rs` `gen_expr` erneut auf `args[0]` aufrief statt die im
  generischen Call-Vorlauf bereits berechneten `arg_vals`/`arg_types`
  wiederzuverwenden — gefixt, Regression-e2e
  `fromcharcode_side_effect.tnx`; mutmaßlich analoge Bugs in anderen
  Builtin-Armen (deleteFile/processExit/dirList/regexFindAll/regexSplit)
  bewusst NICHT mitgefixt (kein Reproduktionsfall, kein riskanter
  Pauschal-Refactor). (3) Eigener Logikfehler (kein Compiler-Bug):
  `AmqpReader091::long()` baute 32-Bit-Werte unsigned auf, negative
  `Int32Val` kamen falsch zurück — `longSigned()` ergänzt. make check grün.
  Nächster Schritt: Phase 3 (Connection/Channel-Handshake).
- 2026-07-22: Phase 3 komplett (AmqpConnection091::connect, AmqpChannel091::
  open, SASL PLAIN, describeAndAckClose-Helper). LIVE gegen RabbitMQ
  3-management (Docker) verifiziert: Erfolgspfad + zwei Negativpfade
  (Auth-Fehler, unbekannter vhost) — beide exakt wie erwartet, Broker-Log
  bestätigt sauberen Verbindungsauf-/-abbau. Automatisierter Loopback-e2e
  `amqp_connection_negotiation.tnx` über `spawn`/`await` (simulierter
  Broker läuft parallel zum blockierenden Client-Code). Neuer Bug gefunden
  (Bug 67, bugs.md, NICHT gefixt): `async fn -> Bool` + `await` verliert
  den Rückgabetyp (immer Int64) — Workaround im Test (Int64 0/1 statt
  Bool). make check grün. Docker-Container `tinox-amqp-test` bleibt für
  Phase 4 (Publish/Consume) laufen. Nächster Schritt: Phase 4.
- 2026-07-23: Phase 4 komplett (declareQueue, bindQueue, publish, consume,
  nextMessage, ack, qos; Beispiel amqp_publish_consume.tnx). Live-Fund:
  `queue.bind` auf den Default-Exchange (`""`) ist laut Spec verboten —
  RabbitMQ schließt den Channel (access_refused) statt eines Silent-Fails,
  danach eskalierte ein weiterer Methodenaufruf auf dem toten Channel
  server-seitig zum Connection-Fehler (die eigentliche Ursache für einen
  Hänger im ersten Testlauf — lag am Testcode, nicht an amqp091.tnx: der
  Rückgabewert von bindQueue wurde ignoriert). Live-Vollablauf (Connect →
  Channel → declareQueue → bindQueue auf `amq.direct` → qos → 2× publish
  über verschiedene Exchanges → consume → 2× nextMessage → ack → close)
  danach fehlerfrei durchgelaufen, Beispiel mit 3-Nachrichten-FIFO-Loop
  ebenfalls live verifiziert. Automatisierter Loopback-e2e
  `amqp_publish_consume_loopback.tnx` (simulierter Broker via spawn,
  spiegelt den Body im simulierten basic.deliver zurück). make check grün.
  Docker-Container `tinox-amqp-test` weiter aktiv. Nächster Schritt: Phase 5
  (Härtung + Abschluss).
- 2026-07-23: Phase 5 komplett — AMQP-0-9-1-Client v1 fertig. 5.1: pika
  (Python) in einem venv installiert, Cross-Check in BEIDE Richtungen live
  gegen RabbitMQ (Tinox->pika und pika->Tinox, je 3 Nachrichten,
  Content-Type korrekt) — bestätigt Wire-Kompatibilität mit einer
  unabhängigen Implementierung. 5.2: leere Bodies + Multi-Frame-Bodies
  (frame-max künstlich auf 100, 250-Byte-Body -> 3 Frames) in beide
  Richtungen getestet (`amqp_edge_cases.tnx`); dabei echten Bug gefunden
  und gefixt — `AmqpWriter091::shortstr()` schrieb die Länge unmaskiert,
  bei > 255 Byte erzeugte `octet()`s `& 0xFF`-Maskierung einen zu kleinen
  Längen-Präfix statt eines harten Fehlers (Silent-Garbage-Verstoß); Fix
  über ein `tooLong`/`hasError()`-Flag, geprüft in
  declareQueue/bindQueue/consume/publish vor dem Senden
  (`amqp_shortstr_limits.tnx`, netzwerkfrei, 255-Byte-Grenzfall zusätzlich
  live verifiziert). Beim Testen einen ZWEITEN, unabhängigen Bug gefunden
  (Bug 68, bugs.md, NICHT gefixt): lange shortstr-Werte + ein
  clientseitig verworfener Aufruf im selben spawn/await-Loopback-Test
  lösten einen nichtdeterministischen Absturz in der Async-Runtime aus —
  nach Bisektion (Docker-Live-Test, isolierte Writer/writeFrame-Tests,
  20+ Wiederholungsläufe pro Variante) als reines Async-Runtime-Problem
  identifiziert, NICHT im AMQP-Code, praktisch unerreichbar im v1-Client.
  Beide neuen e2e-Tests je 20-25× wiederholt lauffähig verifiziert
  (Flakiness ausgeschlossen), bevor sie in den Baum aufgenommen wurden.
  5.3: bugs.md-Abschnitt „Feature: AMQP-0-9-1-Client" (Architektur,
  Härte-Verhalten, bewusste v1-Lücken, Bug-68-Verweis); README.md-
  Abschnitt „AMQP-0-9-1 Client" + Feature-/Modul-Tabellen; docs.html
  `#mod-amqp091`-Sektion (Klassen/Methoden, Beispiel, v1-Lücken) +
  Sidebar + Card-Grid. `make check` grün. Docker-Container
  `tinox-amqp-test` kann jetzt gestoppt werden (nicht mehr für laufende
  Arbeit benötigt). **AMQP-0-9-1-Client-Feature v1 abgeschlossen.**
  Nächster Schritt (falls gewünscht): AMQP 1.0 (eigene Roadmap-Phase, s.
  „Später") oder eines der anderen „Später"-Items (TLS-Client,
  Publisher-Confirms, Multi-Channel, exchange.declare, Annotation-API).
