# Tinox — WebSocket-Roadmap (tasklist.md)

Stand: 2026-07-22. Zweck: Statusübersicht + Wiedereinstiegspunkt über Sessions
hinweg. **Beim Weiterarbeiten: den nächsten offenen Task nehmen (erster `[ ]`
von oben), nach Abschluss hier abhaken und ggf. Notizen ergänzen.**

Status: `[ ]` offen · `[~]` in Arbeit · `[x]` fertig · `[-]` bewusst gestrichen

Arbeitsregeln (Projektkonvention): nach jedem Schritt `make check` komplett
grün; Features/Bugs in bugs.md dokumentieren (Stil: Status/Ursache/Verifiziert);
e2e-Tests unter `tests/e2e/*.tnx` mit `// expect:`-Direktiven; kein
Silent-Garbage — im Zweifel harter Fehler.

---

## Ziel

WebSocket-Server als Sprachfeature/Stdlib-Modul (`crates/tinox-core/websocket.tnx`),
aufgebaut auf der bestehenden Conn-Handle-Architektur des HTTP-Servers
(`httpServerAcceptConnHandle`, `httpConnReadRequest` in `runtime/runtime.c`,
Nutzung siehe `crates/tinox-core/http_server.tnx: listenTls`). Server zuerst;
Client, wss (TLS), Fragmentierung, permessage-deflate sind explizit SPÄTER.

## Befund (Sondierung 2026-07-22, Session-Kontext)

- Kein WebSocket im Repo (Runtime, Stdlib, Docs, Beispiele geprüft).
- Vorhanden: `HttpServer` (epoll, TLS opt-in via `TINOX_TLS=1`),
  `http2_server.tnx`, Raw-TCP-`socket.tnx`, `base64.tnx`.
- **Blocker 1 — SHA-1 fehlt** (`crypto.tnx`/`hash.tnx` haben keins); der
  Handshake braucht `Sec-WebSocket-Accept = base64(sha1(key + GUID))`.
- **Blocker 2 — Binärdaten:** `socketSend`/`socketReceive` und
  `httpConnReadRequest` sind C-String-basiert (`char*`) — WS-Frames enthalten
  NUL-Bytes + Masking, das reißt am ersten `\0` ab. Es braucht LÄNGE-basierte
  Read/Write-Primitiven auf Conn-Handles.
- **Blocker 3 — Upgrade:** der HTTP-Server kennt kein Connection-Upgrade; eine
  Verbindung muss nach dem 101-Handshake aus dem Request/Response-Zyklus
  herausgelöst und als langlebige Frame-Verbindung weitergeführt werden.

---

## Phase 1 — Runtime-Grundlagen (runtime/runtime.c)

- [ ] 1.1 SHA-1 in die C-Runtime (`tinox_sha1(data, len) -> 20 Bytes`), Export
      als Tinox-extern; Wiring in `crypto.tnx` (`Crypto::sha1Hex`,
      `Crypto::sha1Raw` oder als Byte-Array). Testvektoren: leerer String,
      "abc" (a9993e36...), langer Input (>1 Block).
- [ ] 1.2 Binärsichere Conn-Primitiven: `httpConnReadN(conn, n) -> Array<Int64>`
      (oder Byte-Puffer-Handle) und `httpConnWriteBytes(conn, bytes, len)` —
      länge-basiert, kein `char*`-Truncating. Muss auf Plain- UND TLS-Conn-
      Handles funktionieren (dieselbe Dispatch-Stelle wie httpConnReadRequest).
- [ ] 1.3 e2e-Smoke für 1.1/1.2 (SHA-1-Vektoren; Bytes mit \0 durch eine
      Loopback-Conn schleifen).

## Phase 2 — Handshake

- [ ] 2.1 Request-Parsing: GET mit `Upgrade: websocket`, `Sec-WebSocket-Key`
      erkennen (auf dem bestehenden httpConnReadRequest-Pfad — der Handshake
      selbst ist reiner Text, das geht VOR den Binär-Primitiven).
- [ ] 2.2 101-Response bauen: `Sec-WebSocket-Accept` via sha1+base64
      (GUID 258EAFA5-E914-47DA-95CA-C5AB0DC85B11), Antwort roh auf die Conn
      schreiben, Verbindung NICHT schließen.
- [ ] 2.3 Negativ-Pfade hart: fehlender Key / falsche Version (`!= 13`) → 400
      und close. Kein stilles Durchwinken.

## Phase 3 — Frame-Codec (websocket.tnx, pure Tinox über den Byte-Primitiven)

- [ ] 3.1 Frame-Parser: FIN/Opcode/MASK/Payload-Len (7/16/64-bit), Unmasking.
      Client→Server-Frames MÜSSEN maskiert sein (sonst Protokollfehler → close).
- [ ] 3.2 Frame-Serializer (Server→Client, unmaskiert): Text (0x1), Binary
      (0x2), Ping (0x9), Pong (0xA), Close (0x8).
- [ ] 3.3 Control-Frame-Handling: Ping → automatisch Pong; Close → Echo-Close
      + Verbindung schließen. Fragmentierung (FIN=0, Continuation 0x0):
      zunächst NICHT unterstützt → sauberer Close mit 1003/1011 statt Müll,
      in bugs.md als bewusste Lücke dokumentieren.
- [ ] 3.4 Unit-/e2e-Tests: Golden-Frames als Byte-Arrays (kleine/mittlere/
      64-bit-Länge, maskiert/unmaskiert, Ping/Pong/Close).

## Phase 4 — Server-API

- [ ] 4.1 API-Zuschnitt in `websocket.tnx`: `WsServer::new(port)` +
      `onMessage`/`onOpen`/`onClose`-Lambdas ODER Upgrade-Hook am HttpServer —
      Entscheidung dokumentieren (Vorbild: bestehende HttpServer-Handler-API;
      seit 2026-07-22 gibt es Lambdas mit map/filter-Infrastruktur, capturing
      Lambdas funktionieren).
- [ ] 4.2 Accept-Loop: erst blocking-per-Connection (einfach, korrekt);
      epoll-Integration in den bestehenden HTTP-Loop als eigener Folgetask,
      NICHT in v1 mischen.
- [ ] 4.3 Echo-Server als `examples/ws_echo.tnx`.

## Phase 5 — Härtung + Abschluss

- [ ] 5.1 e2e-Test gegen echten Client (Python3 stdlib im Test-Harness oder
      Golden-Byte-Sequenzen über Loopback, wie tests/e2e TLS-Tests es machen —
      prüfen was der Harness hergibt).
- [ ] 5.2 Grenzfälle: Payload 0, 125/126/127-Byte-Grenzen, 64-bit-Länge,
      zerstückelte TCP-Reads (Frame über mehrere reads), Close-Handshake beidseitig.
- [ ] 5.3 bugs.md-Abschnitt (Architektur + bewusste Lücken), README/docs-Eintrag.

## Später (bewusst außerhalb v1)

- [ ] Client-Seite (`WsClient::connect(url)`)
- [ ] wss (TLS) — sollte über die Conn-Handle-Abstraktion fast gratis sein,
      wenn 1.2 auf beiden Handle-Typen sitzt; verifizieren.
- [ ] Fragmentierung/Continuation-Frames
- [-] permessage-deflate (Aufwand/Nutzen, erst bei Bedarf)

---

## Log

- 2026-07-22: Roadmap angelegt (Sondierungsbefund aus Session; noch kein Code).
