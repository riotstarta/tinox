# Tinox — AMQP-1.0-Roadmap (tasklist.md)

Stand: 2026-07-24. Zweck: Statusübersicht + Wiedereinstiegspunkt über Sessions
hinweg. **Beim Weiterarbeiten: den nächsten offenen Task nehmen (erster `[ ]`
von oben), nach Abschluss hier abhaken und ggf. Notizen ergänzen.**

Status: `[ ]` offen · `[~]` in Arbeit · `[x]` fertig · `[-]` bewusst gestrichen

Arbeitsregeln (Projektkonvention): nach jedem Schritt `make check` komplett
grün; Features/Bugs in bugs.md dokumentieren (Stil: Status/Ursache/Verifiziert);
e2e-Tests unter `tests/e2e/*.tnx` mit `// expect:`-Direktiven; kein
Silent-Garbage — im Zweifel harter Fehler.

Vorheriges Feature (AMQP-0-9-1-Client inkl. `amqps://`) ist komplett fertig
und in `bugs.md` archiviert (Suche „Feature: AMQP-0-9-1-Client"). Diese Datei
startet frisch für AMQP 1.0.

---

## Ziel

AMQP-1.0-**Client** (kein Broker), eigenes Modul `tinox.core.amqp10`
(`crates/tinox-core/amqp10.tnx`) — **kein gemeinsamer Code mit
`amqp091.tnx`** außer demselben Transport-Grundmuster (TCP-Dial über
`socketCreateTcp`/`socketConnect`/`httpConnFromFd`/`httpConnFromFdTls`,
binärsicheres Lesen/Schreiben über `httpConnReadN`/`httpConnWriteBytes`).
AMQP 1.0 ist protokoll- und datenmodell-technisch ein komplett anderes Tier
als 0-9-1: eigenes generisches Typsystem (statt fester Feld-Tags), eigener
Frame-Aufbau, eigene SASL-Framing-Schicht, und eine dreistufige
Connection→Session→Link-Hierarchie mit kreditbasierter Flow-Control statt
0-9-1s einfachem Connection→Channel-Modell.

## Sondierung (2026-07-24) — verifizierte Spec-Fakten

Gegen die OASIS-AMQP-1.0-Spec (docs.oasis-open.org, Teile „Types" und
„Transport") sowie den Apache-Qpid-Typ-Referenz-Index geprüft — **nicht**
aus Erinnerung übernommen, weil falsche Descriptor-Codes stillschweigend
inkompatibel mit echten Brokern wären (Wire-Kompatibilität ist bei 0-9-1
der Hauptverifikationsweg gewesen, s. `pika`-Cross-Check, und muss hier
genauso funktionieren):

- **Protokoll-Header** (8 Byte, wie 0-9-1 aber mit Protocol-ID-Byte):
  `"AMQP" 0x00 0x01 0x00 0x00` für die AMQP-Schicht (Protocol-ID 0),
  `"AMQP" 0x03 0x01 0x00 0x00` für die SASL-Schicht (Protocol-ID 3). Bei
  SASL-Auth wird ERST der SASL-Header + SASL-Negotiation gefahren, DANN
  (auf derselben TCP-Verbindung) nochmal der AMQP-Header + die eigentliche
  `open`-Verhandlung — zwei komplette Header-Handshakes hintereinander.
- **Frame-Layout**: `size` (4 Byte, u32 BE, Gesamtgröße inkl. Header) +
  `doff` (1 Byte, Header-Länge in 4-Byte-Worten, Minimum 2 = nur der
  Fixed-Header) + `type` (1 Byte: `0x00` = AMQP-Frame, `0x01` =
  SASL-Frame) + `channel` (2 Byte, u16 BE) + optionaler Extended-Header
  (falls `doff > 2`) + Frame-Body. Deutlich simpler als 0-9-1s
  `octet|short|long|payload|0xCE` nur in der Grobstruktur — der Body ist
  dafür ungleich komplexer (s. u.).
- **Typsystem** (Kernstück, deutlich größer als 0-9-1s Feld-Value-Codec):
  jeder Wert beginnt mit einem 1-Byte-Formatcode, der SOWOHL den Typ ALS
  AUCH oft die Breite/Kodierungsvariante festlegt (z. B. `uint` hat DREI
  Varianten: `0x70` volle 4-Byte-Form, `0x52` `smalluint` 1-Byte-Form für
  0–255, `0x43` `uint0` Null-Byte-Form für den Wert 0 — dieselbe
  Kompression gibt es bei `ulong`/`int`/`long`). Wichtige Codes:
  `null=0x40`, `boolean=0x56`(+`true=0x41`/`false=0x42` als Nullary-Sonderformen),
  `ubyte=0x50`, `ushort=0x60`, `uint=0x70`/`0x52`/`0x43`,
  `ulong=0x80`/`0x53`/`0x44`, `byte=0x51`, `short=0x61`,
  `int=0x71`/`0x54`, `long=0x81`/`0x55`, `float=0x72`, `double=0x82`,
  `char=0x73` (UTF-32BE), `timestamp=0x83` (ms seit Epoch, i64),
  `uuid=0x98`, `binary`: `vbin8=0xa0`(1-Byte-Länge)/`vbin32=0xb0`(4-Byte),
  `string`: `str8-utf8=0xa1`/`str32-utf8=0xb1`, `symbol`(ASCII):
  `sym8=0xa3`/`sym32=0xb3`, `list`: `list0=0x45`(leer)/`list8=0xc0`/`list32=0xd0`,
  `map`: `map8=0xc1`/`map32=0xd1`, `array`: `array8=0xe0`/`array32=0xf0`.
  **Described Types** (für Performatives UND Message-Sections): Formatcode
  `0x00`, gefolgt vom Descriptor-Wert (meist `smallulong`, s. u.) UND
  danach dem eigentlichen Wert (typischerweise ein `list`) — strukturell
  wie ein getaggtes 0-9-1-Field-Table, aber generisch für JEDEN Wert
  nutzbar, nicht nur für Methodenargumente.
  **Falle für den Decoder:** die SASL-Descriptor-Codes (`0x40`–`0x44`)
  überschneiden sich NUMERISCH mit den PRIMITIVEN Formatcodes
  `null`/`boolean-true`/`boolean-false`/`uint0`/`ulong0` — kein echter
  Konflikt im Bytestrom (Descriptor-Codes stehen immer NACH dem
  `0x00`-Described-Type-Präfix, meist zusätzlich hinter einem
  `0x53`-`smallulong`-Marker), aber ein Copy-Paste-Risiko beim Schreiben
  des Decoders, wenn Formatcode- und Descriptor-Tabellen als eine einzige
  Map verwechselt werden.
- **Performative-Descriptor-Codes** (`amqp:<name>:list`, alle als
  `smallulong` kodiert, d. h. `0x53 <code>` im Bytestrom):
  `open=0x10`, `begin=0x11`, `attach=0x12`, `flow=0x13`, `transfer=0x14`,
  `disposition=0x15`, `detach=0x16`, `end=0x17`, `close=0x18`.
  Zusatz-Typen: `error=0x1d`, `source=0x28`, `target=0x29`.
- **SASL-Frame-Descriptor-Codes**: `sasl-mechanisms=0x40`, `sasl-init=0x41`,
  `sasl-challenge=0x42`, `sasl-response=0x43`, `sasl-outcome=0x44`.
- **Message-Section-Descriptor-Codes** (der eigentliche Nachrichtenkörper
  bei `transfer`, mehrere optionale Sections hintereinander im Frame-Body
  nach der `transfer`-Performative): `header=0x70`,
  `delivery-annotations=0x71`, `message-annotations=0x72`,
  `properties=0x73`, `application-properties=0x74`, `data=0x75`
  (Rohbytes-Body), `amqp-sequence=0x76`, `amqp-value=0x77` (ein einzelner
  AMQP-Wert als Body), `footer=0x78`.
- **Connection→Session→Link statt Connection→Channel**: `open`/`close`
  auf Connection-Ebene (wie 0-9-1), aber darüber `begin`/`end` für eine
  **Session** (fenster-basierte Flow-Control, `incoming-window`/
  `outgoing-window`, ersetzt 0-9-1s reines "ein Channel ist offen"), und
  darüber `attach`/`detach` für einen **Link** (explizite, langlebige
  Sender- ODER Receiver-Rolle mit `source`/`target`-Adressen — kein
  Äquivalent in 0-9-1, wo `basic.consume`/`basic.publish` implizit auf
  dem Channel laufen). Nachrichtenfluss läuft NICHT wie 0-9-1 unbegrenzt,
  sondern kreditbasiert: `flow` vergibt `link-credit`, der Sender darf nur
  so viele `transfer`s schicken, wie Credit vorhanden ist — fehlt in
  0-9-1 komplett (dort regelt nur `basic.qos`/`prefetchCount` grob den
  Consumer-seitigen Puffer, kein echtes Fenster-Protokoll).
- **Wiederverwendbar aus `amqp091.tnx` (Muster, nicht Code):** Conn-Handle-
  Dial (`socketCreateTcp`+`socketConnect`+`httpConnFromFd`/`httpConnFromFdTls`
  für `amqps`-Äquivalent), `httpConnReadN`/`httpConnWriteBytes` als
  binärsichere Primitiven, der grundsätzliche "harter Fehler statt Silent-
  Garbage"-Stil (`errorMessage`-Feld + `.ok`/leerer-Rückgabewert-Konvention).
- **Live-Verifikation:** RabbitMQ braucht das `rabbitmq_amqp1_0`-Plugin
  aktiviert (`rabbitmq-plugins enable rabbitmq_amqp1_0`) für 1.0-Support —
  im v0-9-1-Docker-Container noch nicht aktiviert, muss vor Phase-5-Live-
  Tests nachgerüstet werden. Alternative: Apache Qpid Proton/Broker-J oder
  ActiveMQ Artemis (native AMQP-1.0-Unterstützung, kein Plugin nötig) —
  Entscheidung bei Phase 5 treffen, je nachdem was zu diesem Zeitpunkt
  lokal am einfachsten verfügbar ist. Kein `python-qpid-proton` (der
  naheliegende unabhängige Cross-Check-Client analog zu `pika` bei 0-9-1)
  installiert — bei Bedarf `pip install python-qpid-proton` in einem venv.

---

## Phase 1 — Transport + Protokoll-Handshake (`crates/tinox-core/amqp10.tnx`) ✅

- [x] 1.1 Modul-Skelett `tinox.core.amqp10`. `Amqp10::dial(host, port) ->
      Int64` (Plaintext) + `dialTls(host, port, verify: Bool) -> Int64`
      (analog zu `Amqp091::dial`/`dialTls` — eigene, unabhängige
      Implementierung im neuen Modul, kein Cross-Import zwischen den
      beiden AMQP-Modulen).
- [x] 1.2 AMQP-Protokoll-Header-Handshake: `sendAmqpProtocolHeader` sendet
      `"AMQP" 0x00 0x01 0x00 0x00`. `checkProtocolHeaderEcho(conn, sent)`
      liest 8 Byte und vergleicht sie EXAKT mit dem gesendeten Header —
      anders als 0-9-1 (Erfolg = erstes Byte eines echten Frames) ist bei
      1.0 Erfolg das exakte Echo desselben Headers (§2.2). Ungleicher
      Header (Broker schlägt andere Version vor) ODER EOF/Kurzread beide
      harter Fehlschlag; feinere Versions-Mismatch-Behandlung (Broker-
      Vorschlag auswerten statt nur "kein Match") ist ein Later-Punkt.
- [x] 1.3 SASL-Protokoll-Header-Handshake: `sendSaslProtocolHeader` sendet
      `"AMQP" 0x03 0x01 0x00 0x00`. Getrennte Methode von 1.2, da beide
      Header nacheinander auf DERSELBEN Verbindung verschickt werden (erst
      SASL-Header+Negotiation, danach AMQP-Header+`open`) — `checkProtocolHeaderEcho`
      wird für beide Varianten wiederverwendet (nimmt den gesendeten Header
      als Parameter statt ihn zu hardcoden).
- [x] 1.4 e2e-Test `tests/e2e/amqp10_phase1_handshake.tnx`: dial-refused,
      Header-Bytes-Assertion für AMQP- UND SASL-Header, Erfolgs-/Mismatch-/
      EOF-Pfad für den Header-Echo-Check — Muster wie
      `amqp_phase1_handshake.tnx` (0-9-1), 10× stabil wiederholt.
      stdlib_smoke-Eintrag `amqp10` ergänzt (dial gegen unbelegten Port).

## Phase 2 — Typsystem-Codec (Kernstück) ✅

- [x] 2.1 `Amqp10Writer`: primitive Encoder-Methoden für alle Fixed-Width-
      Typen (`null`, `boolean`, `ubyte`, `ushort`, `uint`, `ulong`, `byte`,
      `short`, `int`, `long`, `char`, `timestamp`, `uuid`) — v1 nutzt
      bewusst NUR die volle Breite beim Schreiben (kein
      `smalluint`/`smallulong`/`uint0`/etc.-Kompressions-Picking, reine
      Größenoptimierung, kein Korrektheitserfordernis); der DECODER liest
      trotzdem ALLE Kurzformen, weil der Broker sie nutzen darf.
      **v1-Lücke (bewusst, s. `Amqp10Value`-Kommentar):** `float`/`double`/
      `decimal32/64/128` werden NICHT kodiert/dekodiert (kein
      IEEE-754-Bitcast-Primitive in der Runtime, und die Kern-Performatives
      nutzen laut Spec nirgends float/double) — der Decoder erkennt die
      Formatcodes und überspringt sie korrekt (Cursor bleibt synchron),
      liefert aber `ErrorVal` statt eines echten Werts.
- [x] 2.2 `Amqp10Writer`: variable-width Typen (`binary`/`string`/`symbol`)
      — **Design-Abweichung von der ursprünglichen Planung:** kein
      8-vs-32-Bit-Cutover im Writer. Die 32-Bit-Form (`vbin32`/`str32`/
      `sym32`) ist für JEDE Länge spec-konform (kostet nur ein paar Bytes
      mehr bei kleinen Werten) — der Writer schreibt sie IMMER, kein
      Größen-Zweig, keine künstliche Fehlergrenze wie bei 0-9-1s
      255-Byte-`shortstr`-Limit. Der Reader muss trotzdem beide Formen
      lesen können. Dieselbe Vereinfachung gilt für List/Map (immer
      `list32`/`map32`, nie `list0`/`list8`/`map8`).
- [x] 2.3 `Amqp10Writer`/`Amqp10Reader`: compound Typen (`list`/`map`) +
      **Array-Decoding** (`array8`/`array32`, ursprünglich nicht explizit
      geplant, aber nötig — Broker-Felder wie `sasl-server-mechanisms`
      oder `offered-capabilities` sind `symbol`-Arrays, die Phase 4/5
      lesen müssen können) — rekursiv über `Amqp10Value`-Enum (analog zu
      0-9-1s `AmqpFieldValue091`, aber generischer: AMQP 1.0 nutzt dieses
      EINE Typsystem für Performatives, Message-Sections UND Feld-Tables).
      `MapVal` ist eine FLACHE `List<Amqp10Value>` (key,val,key,val,...)
      statt `Map<K,V>`, weil AMQP-1.0-Map-Keys beliebige Werte sein dürfen
      (kein einheitlicher K-Typ wie bei 0-9-1s Immer-String-Keys).
      Described-Type-Encoding/-Decoding (`0x00` + Descriptor + Wert) als
      generischer Baustein für Phase 3 (Performatives) + Phase 6
      (Message-Sections). `Amqp10Described`-Klasse: v1 beschränkt den
      Descriptor auf `Int64` (numerisch, als `smallulong` kodiert) — deckt
      alle Performatives/SASL-Frames/Sections ab (alle nutzen laut Spec
      numerische Descriptoren), Symbol-Descriptoren sind ein v1-Nicht-Ziel.
- [x] 2.4 (in 2.3 gelandet, s. o.) — `Amqp10Reader`-Decoder für ALLE
      Formatcode-Varianten inkl. der komprimierten Kurzformen.
- [x] 2.5 e2e-Test `tests/e2e/amqp10_typesystem.tnx`: Rundtrip
      Encode/Decode für jeden unterstützten Formatcode (30 Prüfungen) +
      verschachtelte List/Map-Kombinationen + Kurzformen, die NUR der
      Reader kennen muss (von Hand gebaute Bytes, da der Writer sie nie
      schreibt) + Array-Decoding + Fehlerpfade (`float`, unbekannter
      Formatcode). 10× stabil wiederholt.
      **Echter Compiler-Bug gefunden + gefixt (Bug 69, bugs.md):**
      Enum-Diskriminatoren wurden als Zeichensummen-Checksumme des
      Variantennamens berechnet — `"UShortVal"` und `"BinaryVal"`
      kollidierten (beide summieren auf 904), wodurch `match` die falsche
      Sibling-Variante traf (Silent-Garbage, kein Fehler). Gefixt in
      `codegen.rs` mit FNV-1a-Hash statt Zeichensumme (vier Call-Sites).
      **Kompletter `make check`-Lauf grün** (kritisch, weil der Fix JEDES
      Enum im gesamten Projekt betrifft, nicht nur AMQP 1.0).

## Phase 3 — Frame-Codec + Performative-Hülle ✅

- [x] 3.1 `Amqp10Frame`-Klasse (`frameType`/`chanId`/`body` — Feld hieß
      ursprünglich `channel`, kollidiert wie bei 0-9-1 mit dem
      Concurrency-Keyword, umbenannt zu `chanId`). `Amqp10::readFrame`/
      `writeFrame`: `size|doff|type|chanId|[extended-header]|body`. `size`
      ist die GESAMTgröße inkl. der 8 Fixed-Header-Bytes (anders als
      0-9-1s `long`, das nur die Payload-Länge zählt). v1 schreibt `doff`
      immer `2` (kein Extended Header), liest aber `doff > 2` korrekt und
      überspringt die zusätzlichen Bytes (Broker darf ihn nutzen).
      Harte Protokollfehler (`frameType -2`) bei `size < 8`, `doff < 2`,
      `doff*4 > size` oder Payload > 16-MB-Cap — analog zu 0-9-1s
      Frame-Terminator-Check, kein Silent-Garbage.
- [x] 3.2 `Amqp10Performative`-Klasse (`descriptor`/`fields`) +
      `Amqp10::encodePerformative(descriptorCode, fields: List<Amqp10Value>)
      -> List<Int64>`/`decodePerformative(body) -> Amqp10Performative` —
      EIN generischer Baustein für alle 9 Performatives (Described Type
      über einer `ListVal`), anders als 0-9-1s methodenspezifisches
      `encodeMethodPayload` (classId+methodId+eigenes Argument-Schema pro
      Methode). Bei unerwarteter Struktur (kein Described Type, Wert ist
      keine Liste) hart `descriptor: -1` statt Best-Effort-Parsing.
      **Rückgabetyp-Korrektur:** ursprünglich als Tuple `(Int64,
      List<Amqp10Value>)` geplant — Tuple-Rückgabetypen sind im Projekt
      an keiner Stelle genutzt/getestet (nur Tuple-Literale + `.0`/`.1`-
      Zugriff), also stattdessen eine Klasse mit benannten Feldern
      (bewährtes Muster, kein Risiko eines unentdeckten Codegen-Lecks).
- [x] 3.3 e2e-Test `tests/e2e/amqp10_frame_codec.tnx`: In-memory
      Performative-Rundtrip (einzelnes + gemischt-typisiertes Multi-Feld)
      + kaputte Struktur (rohes `UIntVal` statt Described Type) + echter
      Frame-Rundtrip über Loopback-Conn + Extended-Header-Skip
      (AMQP-1.0-spezifisch, kein 0-9-1-Äquivalent) + alle vier
      Protokollfehler-Fälle + EOF. 10× stabil wiederholt.

## Phase 4 — SASL-Negotiation ✅

- [x] 4.1 `sasl-mechanisms` (Descriptor `0x40`) empfangen — Broker bietet
      Mechanismen als `symbol-array` an (laut Spec immer ein Array, auch
      bei genau einem Mechanismus); v1 sucht `PLAIN` darin, sonst harter
      Fehler (analog zu 0-9-1s `mechanisms.contains("PLAIN")`-Check).
      Zusätzlich ein `SymbolVal`-Fallback für den (spec-widrigen, aber
      defensiv abgefangenen) Fall eines nackten Einzelwerts ohne
      Array-Hülle.
- [x] 4.2 `sasl-init` (`0x41`) senden: `SymbolVal("PLAIN")` +
      `BinaryVal(saslPlainResponse(user, pass))` (dieselbe
      SASL-PLAIN-Byte-Struktur `\0user\0pass` wie bei 0-9-1 —
      `Amqp10::saslPlainResponse`, neu implementiert, kein Cross-Import
      zwischen den AMQP-Modulen) + `NullVal` für das optionale
      `hostname`-Feld.
- [x] 4.3 `sasl-outcome` (`0x44`) empfangen, `code`-Feld (`UByteVal`)
      prüfen (0 = ok, alles andere = harter Auth-Fehler mit
      `errorMessage`, die den Code nennt). NICHT unterstützt:
      `sasl-challenge`/`sasl-response`-Runden (nur für Mechanismen wie
      SCRAM nötig, v1 bleibt bei PLAIN).
      `Amqp10::negotiateSasl(conn, user, pass) -> Amqp10SaslResult`
      bündelt 4.1-4.3 zu einem wiederverwendbaren Baustein für Phase 5.
- [x] 4.4 e2e-Test `tests/e2e/amqp10_sasl_negotiation.tnx` (simulierter
      Broker via `spawn`/`await`, Muster wie
      `amqp_connection_negotiation.tnx`): erfolgreiche Negotiation
      (inkl. Prüfung, dass der Broker tatsächlich `PLAIN` im `sasl-init`
      sieht) UND abgelehnte Auth (`sasl-outcome` code = 1) → sauberer
      Fehler mit dem Code in `errorMessage`. 20× stabil wiederholt
      (Bug-68-Vorsicht, da `spawn`/`await` beteiligt ist).

## Phase 5 — Connection/Session/Link-Handshake ✅

- [x] 5.1 `Amqp10Connection::connect(host, port, user, pass) ->
      Amqp10Connection`: SASL-Header+Negotiation (Phase 4), dann
      AMQP-Header (1.2) + `open` senden + `open` vom Broker empfangen.
      Feldpositionen container-id(0)/hostname(1)/max-frame-size(2)/
      channel-max(3) — v1 übernimmt die Server-Vorschläge (trailing
      Felder dürfen in der Antwort fehlen, dann gilt der Default weiter),
      kein eigenes Verhandeln, analog zu 0-9-1s `connection.tune`.
- [x] 5.2 `Amqp10Session::begin(connection) -> Amqp10Session`: `begin`
      senden (`remote-channel`=null, `next-outgoing-id`=0,
      `incoming-window`/`outgoing-window` großzügig fest — Phase 6 macht
      echtes Fenster-Tracking) + `begin` vom Broker empfangen. v1: fester
      Kanal 1 pro Verbindung (kein Pool), analog zur 0-9-1-v1-Entscheidung.
- [x] 5.3 `Amqp10Link::attach(session, name, role, address) ->
      Amqp10Link`: `attach` senden (Rolle Sender=false/Receiver=true,
      `source`/`target`-Described-Type je nach Rolle — die jeweils
      andere Seite bekommt eine leere, adresslose Hülle statt `NullVal`)
      + `attach` vom Broker empfangen. v1: ein Link pro Zweck, fester
      `handle` 0 (kein Multi-Link-Pool pro Session).
      **Echter Bug gefunden + gefixt (live gegen RabbitMQ 4.x, s. u.):**
      `initial-delivery-count` (Performative-Feld 10) ist per Spec
      PFLICHT, wenn `role=Sender` — ursprünglich weggelassen, RabbitMQs
      `rabbit_amqp_session:handle_attach` pattern-matched das Feld hart
      auf einen echten `uint`-Wert und crasht mit `function_clause`, wenn
      es fehlt. Gefixt: `unsettled`(8, `NullVal`)/
      `incomplete-unsettled`(9, `false`) als Platzhalter davor +
      `initial-delivery-count`(10, `0`) — Listenfelder dürfen nur am ENDE
      weggelassen werden, nicht in der Mitte.
- [x] 5.4 Sauberes Schließen: `detach` (Link) → `end` (Session) → `close`
      (Connection), jeweils auf die Gegenstück-Performative vom Broker
      warten (v1 ungeprüft, analog zu 0-9-1s `close`/`close-ok`-Pattern,
      hier aber DREI Ebenen statt einer).
- [x] 5.5 e2e-Test `tests/e2e/amqp10_connection_session_link.tnx`
      (simulierter Broker, `spawn`/`await`): kompletter
      Open→Begin→Attach→Detach→End→Close-Ablauf inkl. beider
      Header-Handshakes (SASL dann AMQP) davor, Verifikation dass der
      Broker `role=Sender` im `attach` sieht. 15-20× stabil wiederholt
      (Bug-68-Vorsicht).
- [x] 5.6 **Live gegen echten Broker verifiziert** (RabbitMQ 4.x,
      `rabbitmq:4-management` — hat AMQP 1.0 nativ im Core-Broker, kein
      Plugin mehr nötig ab 4.0). **RabbitMQ 3.x mit dem alten
      `rabbitmq_amqp1_0`-Plugin bewusst NICHT als Referenz genutzt:** der
      Broker selbst crasht dort mit `function_clause` in
      `rabbit_amqp1_0_incoming_link:attach` bei genau demselben,
      spec-konformen Attach-Frame, das RabbitMQ 4.x klaglos verarbeitet —
      ein Bug/eine Einschränkung des alten Legacy-Plugins, kein
      Client-Bug (per Log bestätigt: der Frame wurde korrekt dekodiert,
      der Crash liegt in Brokers eigenem Pattern-Match). Verifiziert:
      voller Connect→Begin→Attach(Sender)→Detach→End→Close-Ablauf sauber
      (`closing AMQP connection`-Log ohne Warnung), zusätzlich
      Attach(Receiver) gegen eine per Management-API angelegte Test-Queue.
      **Zwei weitere Live-Erkenntnisse dokumentiert (s. bugs.md):**
      RabbitMQ 4.x verlangt „AMQP Address v2" (`/queues/name`,
      `/exchanges/name/routing-key` — die zuvor genutzte `/queue/name`-
      Form ist deprecatet und wird mit `amqp_address_v1_not_permitted`
      abgelehnt); der Broker schickt nach einem erfolgreichen
      Sender-Attach unaufgefordert ein `flow`-Frame (Credit-Grant) —
      relevant für Phase 6, das dort einen Frame-Dispatcher statt reinem
      Request/Response-Lockstep braucht (v1s `close`/`end`/`detach`
      prüfen die Antwort ohnehin nicht, deshalb hier noch kein sichtbares
      Symptom, aber ein wichtiger Design-Hinweis für Phase 6).

## Phase 6 — Transfer/Flow/Disposition (Publish/Consume) ✅

- [x] 6.1 `Amqp10Link::awaitFlow()` (liest ein `flow`, aktualisiert
      `this.linkCredit` aus Feld 6) + `grantCredit(amount)` (sendet
      `flow`, Receiver-seitig) — das kreditbasierte Gegenstück zu 0-9-1s
      `basic.qos`, aber hier zwingend für JEDEN Transfer nötig (kein
      Transfer ohne Credit erlaubt, im Gegensatz zu 0-9-1, wo Publish
      immer sofort geht). `Amqp10Session`/`Amqp10Link` um `maxFrameSize`/
      `linkCredit`/`deliveryCount` erweitert (durchgereicht von
      `Amqp10Connection`).
- [x] 6.2 `Amqp10::encodeMessageBody(body, contentType)`: `properties`-
      Section (0x73, nur `content-type` gesetzt, 6 `NullVal`-Platzhalter
      davor) + `data`-Section (0x75, Rohbytes) — analog zum 0-9-1-
      Content-Type/Body-Konzept, aber als eigene Described-Type-Sections
      statt eines Content-Header-Frames + separater Body-Frames. Kein
      `header`/`application-properties` in v1.
- [x] 6.3 `Amqp10Link::publish(body, contentType)`: Performative +
      Message-Sections im selben Frame-Body (kein separates Content-
      Header-Frame wie bei 0-9-1). Wartet via `awaitFlow()` auf Credit,
      falls erschöpft. Bei Überschreiten von `max-frame-size` (grober,
      sicherer 64-Byte-Overhead-Puffer für die Transfer-Hülle) auf
      mehrere `transfer`-Frames mit `more=true` aufgeteilt — funktionales
      Äquivalent zu 0-9-1s Multi-Frame-Body-Splitting, anderer
      Mechanismus (`more`-Flag statt impliziter Fortsetzung durch
      Frame-Grenzen). v1: `settled=true` (fire-and-forget wie 0-9-1s
      v1-Publish ohne Publisher-Confirms).
- [x] 6.4 `Amqp10Link::nextMessage()` (empfängt `transfer`, dekodiert
      `data`- ODER `amqp-value`-Body-Section, s. Live-Cross-Check-Fund
      unten) + `ack(deliveryId)` (sendet `disposition` mit
      `state=accepted`, `settled=true`) — AMQP-1.0-Äquivalent zu 0-9-1s
      `basic.ack`, mit granularerem Delivery-State-Modell (v1 nutzt nur
      `accepted`, s. „Später" für rejected/released/modified). v1
      reassembliert KEINE Multi-Frame-Transfers beim Empfang (bewusste
      Lücke, s. „Später") — `more=true` wird erkannt und als Fehler
      gemeldet statt nur den ersten Teil zurückzugeben.
- [x] 6.5 e2e-Test `tests/e2e/amqp10_transfer_flow.tnx` (simulierter
      Broker, `spawn`/`await`): Publish/Consume-Roundtrip inkl.
      Multi-Frame-Transfer (Broker setzt gesplittete Transfers wieder
      zusammen) UND Credit-Erschöpfung (Broker vergibt nur 1 Credit,
      zweiter `publish()`-Aufruf muss intern korrekt auf ein neues `flow`
      warten, bevor er sendet — verifiziert über einen Marker, den der
      Broker erst nach dem zweiten `flow` sieht). 25× stabil wiederholt
      (Bug-68-Vorsicht, `spawn`/`await` + mehrere Attach/Detach-Zyklen).
- [x] 6.6 **Live gegen echten RabbitMQ-4.x-Broker verifiziert**
      (Publish→Consume→Ack→sauberes Schließen über eine echte Queue) UND
      **Live-Cross-Check mit `python-qpid-proton`** (unabhängiger
      Client) in BEIDE Richtungen: Tinox publiziert → Proton konsumiert,
      UND Proton publiziert → Tinox konsumiert.
      **Echter Interop-Bug gefunden + gefixt (bugs.md):** Proton kodiert
      einen einfachen String-Body NICHT als `data`-Section (0x75,
      Rohbytes), sondern als `amqp-value`-Section (0x77) mit einem
      `StrVal` darin — `nextMessage()` kannte nur `data` und lieferte
      dafür `ok=true` mit LEEREM Body zurück (Silent-Garbage). Gefixt:
      `amqp-value` mit `StrVal`/`BinaryVal` wird jetzt ebenfalls dekodiert,
      UND `nextMessage()` liefert `ok=false`, wenn gar keine erkannte
      Body-Section gefunden wurde, statt still einen leeren Body zu
      melden.

## Phase 7 — Härtung + Abschluss

- [ ] 7.1 Grenzfälle analog zu 0-9-1 Phase 5.2: leere Message-Bodies,
      Multi-Frame-Transfer-Grenzen, `str8`/`sym8`-255-Byte-Grenze
      (Pendant zu 0-9-1s `shortstr`-Limit — hier aber mit `str32`/`sym32`
      als eingebautem Fallback statt hartem Fehler, da AMQP 1.0 selbst
      schon zwei Größenklassen kennt; prüfen ob v1 dennoch nur `str8`/
      `sym8` schreibt und > 255 Byte hart ablehnt, um den Encoder simpel
      zu halten, oder ob automatisches Umschalten auf die 32-Bit-Form
      sinnvoller ist — Design-Entscheidung bei Phase 7, nicht vorab
      festgelegt).
- [ ] 7.2 bugs.md-Abschnitt „Feature: AMQP-1.0-Client" (Architektur +
      Vergleich zu 0-9-1 + bewusste Lücken, Stil wie bei den vorherigen
      Features). README.md + docs.html analog zu 0-9-1 ergänzen
      (`amqp10` in der Messaging-Kategorie neben `amqp091`).

---

## Später (bewusst außerhalb v1)

- [ ] Mehrere Sessions/Links pro Connection (v1: je genau einer).
- [ ] `sasl-challenge`/`sasl-response`-Mehrrunden-Mechanismen (z. B.
      SCRAM) — v1 nur PLAIN.
- [ ] Delivery-State jenseits von `accepted` (`rejected`/`released`/
      `modified`) auf Sender-seitiger Auswertung.
- [ ] Transaktionen (`txn-id`, `declare`/`discharge`).
- [ ] Link-Recovery/Resumption (`unsettled`-Map beim `attach`, für
      Exactly-Once-Semantik über Reconnects hinweg).
- [ ] Annotation-getriebene Consumer-API (analog zur WS-`@OnMessage`-
      Idee) — erst wenn Lambda-als-Methodenparam zuverlässig gedeckt ist
      (dieselbe Baustelle wie bei 0-9-1 und WS v1 vermerkt).
- [ ] Heartbeat (`empty` AMQP-Frame als Keepalive) / Auto-Reconnect.
