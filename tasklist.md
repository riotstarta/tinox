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

- [x] 1.1 SHA-1 in die C-Runtime (`sha1_raw` + `sha1Hash` hex, Stil wie
      sha256), `Crypto::sha1` in crypto.tnx. ZUSÄTZLICH `wsAcceptKey(key)`
      komplett in C (sha1+base64 inkl. eigenem `tinox_b64_encode` über
      Rohbytes) — der binäre Digest muss so nie durch einen Tinox-String.
- [x] 1.2 Binärsichere Conn-Primitiven: `httpConnReadN(conn, n) -> List<Int64>`
      (liest EXAKT n, blockierend; kürzer = EOF/Fehler, Aufrufer prüft Länge)
      und `httpConnWriteBytes(conn, bytes) -> Int64`; ein Byte pro i64-Slot.
      Läuft via conn_recv/conn_send_all auf Plain- UND TLS-Handles. Dazu
      `httpConnFromFd(fd)`: wickelt nackte Socket-fds (Client-Seite) in ein
      Conn-Handle — für Tests und später WsClient.
- [x] 1.3 e2e `tests/e2e/ws_phase1_primitives.tnx`: SHA-1-Vektoren (leer, abc,
      128×a), wsAcceptKey gegen RFC-6455-Beispiel, NUL-Bytes-Loopback in beide
      Richtungen (connect-vor-accept, single-threaded via Backlog, Port 47613).

## Phase 2 — Handshake ✅ (`crates/tinox-core/websocket.tnx`, `Ws::handshake`)

- [x] 2.1 Request-Parsing: Header zeilenweise, case-insensitiv via
      toLowerCase+startsWith (`sec-websocket-key`, `-version`, `upgrade`).
- [x] 2.2 101-Response mit `Sec-WebSocket-Accept` via `wsAcceptKey` (C-seitig,
      Phase 1); Verbindung bleibt offen.
- [x] 2.3 Negativ-Pfade: fehlender Key / Version != 13 / kein Upgrade-Header →
      400 + false; `WsServer::accept` schließt dann und gibt -1.

## Phase 3 — Frame-Codec ✅ (websocket.tnx, pure Tinox über den Byte-Primitiven)

- [x] 3.1 `Ws::readFrame`: FIN/Opcode/MASK/Len (7/16/64-bit), Unmasking;
      unmaskierte Client-Frames + gesetzte RSV-Bits → opcode -2
      (Protokollfehler); EOF/kurzer Read → opcode -1; Payload-Cap 16 MB.
- [x] 3.2 `Ws::writeFrame` (unmaskiert, FIN=1) + sendText/sendBinary/sendPing/
      sendClose; `Ws::text`/`textToBytes` (byte-basiert — UTF-8 bleibt als
      Byte-Roundtrip erhalten, live gegen python-websockets verifiziert).
- [x] 3.3 `Ws::readMessage`: Ping → auto-Pong (Payload gespiegelt), Pong wird
      geschluckt, Close → Echo-Close + Rückgabe (Aufrufer schließt);
      Continuation/FIN=0 → Close 1003, Protokollfehler → Close 1002.
- [x] 3.4 e2e `tests/e2e/ws_handshake_frames.tnx`: Golden-Frames (maskiert,
      handgebaut) über echte Loopback-Conn — Text klein, Ping→Pong, 126er-Pfad
      beidseitig (200 B), Close-Echo, Handshake-Ablehnung. Dazu stdlib_smoke-
      Eintrag (reine Codec-Logik).

## Phase 4 — Server-API ✅ (v1: explizite Schleifen-API)

- [x] 4.1 ENTSCHEIDUNG: explizite API (`WsServer::listen/accept` +
      `Ws::readMessage`-Schleife) statt Lambda-Handler — Lambdas als Params
      von USER-Methoden sind noch nicht gedeckt (nur Builtin-Array-Methoden,
      s. map/filter-Lücken in bugs.md); Handler-API als Folgetask, wenn das
      Fundament da ist.
- [x] 4.2 Accept-Loop blocking-per-Connection; epoll-Integration bewusst
      NICHT in v1 (eigener Folgetask, s. „Später").
- [x] 4.3 `examples/ws_echo.tnx` (Port 8790; 8090 ist auf der Dev-Maschine
      von einem Fremddienst belegt).

## Phase 5 — Härtung + Abschluss

- [x] 5.1 Live-Test gegen ECHTEN unabhängigen Client (python websockets
      16.1.1) manuell gefahren: Handshake, Text-Echo, UTF-8-Roundtrip
      (ümläut-tęst), lib-Ping/Pong, 2 sequentielle Verbindungen, 5 KB
      (126er-Pfad), 70 KB (127er/64-bit-Pfad beidseitig), Binary mit NULs —
      alles grün. NICHT als automatischer e2e (bräuchte python-websockets als
      Harness-Abhängigkeit); Golden-Frame-e2e deckt den Codec ab.
- [~] 5.2 Grenzfälle: 126er beidseitig + Close-Handshake im e2e; 127er-Pfad
      nur live (im e2e würden >64 KB single-threaded die Loopback-Puffer
      riskieren). OFFEN: expliziter e2e für Payload 0 + 125/126-Grenze exakt,
      zerstückelte TCP-Reads (readN-Loop deckt das konstruktiv, ungetestet).
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
- 2026-07-22: Phase 1 komplett (SHA-1, wsAcceptKey, ReadN/WriteBytes/FromFd,
  e2e-Smoke). Nebenbei: vorbestehende Clippy-Lint in main.rs:866 gefixt (kam
  durch Cache-Invalidierung hoch). make check grün.
- 2026-07-22: Phasen 2-4 komplett + 5.1 (websocket.tnx: Handshake, Frame-Codec,
  readMessage-Loop, WsServer, examples/ws_echo.tnx; e2e ws_handshake_frames +
  stdlib_smoke-Eintrag; live gegen python-websockets 16.1.1 verifiziert inkl.
  UTF-8, 70-KB-127er-Pfad, Binary mit NULs). Offen: 5.2-Rest, 5.3.
