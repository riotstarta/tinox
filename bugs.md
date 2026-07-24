# Tinox Compiler Bugs — Offene Punkte

Diese Datei enthält nur noch, was aktuell **nicht** erledigt ist. Das
vollständige historische Log aller gefundenen und gefixten Bugs sowie
abgeschlossener Feature-Implementierungen (Root Cause, Fix, Verifikation,
verworfene Ansätze) steht in **`bugs_fixed.md`** — dorthin verweist jeder
Punkt hier für den vollen Kontext.

Stand: 2026-07-25. Wenn einer dieser Punkte gefixt wird: Eintrag hier
entfernen, vollständige Fix-Doku (Root Cause + Fix + Verifiziert, Stil wie
in `bugs_fixed.md`) dort ergänzen.

---

## Sub-Statement-Granularität beim throw-Unwinding

`throw`-Propagierung (s. `bugs_fixed.md`, Bug 40) läuft auf
**Statement**-Granularität: nach jedem Statement, das werfen könnte, wird
geprüft und ggf. propagiert. Bei einem zusammengesetzten Ausdruck mit
mehreren Calls in EINEM Statement — z. B. `a() + b()` — läuft `b()` noch,
auch wenn `a()` bereits geworfen hat; die Propagierung greift erst an der
nächsten Statement-Grenze.

Für die Praxis (Zwischen-Frames, Schleifen, Rückgabewerte — der weit
überwiegende Teil realer Fehlerpfade) ist Statement-Granularität
vollständiges, sofortiges Unwinding; nur echte Sub-Statement-Immediacy in
zusammengesetzten Ausdrücken fehlt. Ein Fix bräuchte Post-Call-Checks nach
JEDEM Teilausdruck (nicht nur nach Statements) oder eine setjmp/longjmp-
basierte Unwinding-Architektur (bewusst nicht gewählt, s. Bug 40 in
`bugs_fixed.md` — großer Blast-Radius wegen Handler-Stack-Aufräumen bei
`return`/`break`/`continue`/`finally`/`defer`).

**Kontext/Historie:** `bugs_fixed.md`, Bug 40.

---

## Enum-Diskriminator global statt pro-Enum geschlüsselt

Enum-Varianten sind laut bestehendem Design GLOBAL nach Namen geschlüsselt
(nicht pro Enum, s. Kommentar bei `register_variant_payloads` in
`codegen.rs`). Der Bug-69-Fix (FNV-1a-Hash statt Zeichensummen-Checksumme)
macht Diskriminator-Kollisionen zwischen zwei Varianten unterschiedlicher
Enums nur noch „astronomisch unwahrscheinlich" (für kurze,
von Menschen gewählte Bezeichnernamen) statt sie strukturell
auszuschließen.

Ein Diskriminator, der zusätzlich den Enum-Namen mit einbezieht (härtere,
aber invasivere Lösung), würde eine neue Registry-Infrastruktur brauchen,
die es noch nicht gibt — bewusst nicht gebaut, um den akuten
Kollisionsfund (Bug 69) mit einer minimalen, gut abgegrenzten Änderung zu
schließen statt einen größeren, riskanteren Umbau zu erzwingen. Ist damit
eine bekannte, dokumentierte Design-Grenze, kein akuter Bug — nur relevant,
falls jemals zwei Enum-Varianten (in unterschiedlichen Enums) denselben
FNV-1a-Hash treffen sollten.

**Kontext/Historie:** `bugs_fixed.md`, Bug 69.

---

## TLS-Tests (HTTPS/WSS/AMQPS) nicht automatisiert

Drei TLS-Features (`HttpServer::listenTls`, `WsServer::listenTls`,
`AmqpConnection091::connectTls`) sind jeweils nur **manuell** gegen ein
selbstsigniertes Testzertifikat verifiziert (inkl. des Negativpfads:
`verify=true` schlägt korrekt mit `certificate verify failed` fehl), nicht
über einen committeten, in `make check` laufenden e2e-Test. Grund: ein
solcher Test bräuchte ein Testzertifikat als Fixture im Repo.

Kein Korrektheitsrisiko (die Funktionalität selbst ist verifiziert), aber
eine Lücke in der automatisierten Regressionsabsicherung — ein künftiger
TLS-Regressions-Bug in diesem Bereich würde `make check` nicht auffangen.

**Kontext/Historie:** `bugs_fixed.md`, Feature 34 (HTTPS), WebSocket-Server
(WSS-Nachtrag), AMQP-0-9-1-Client (`amqps://`-Nachtrag).

---

## `Crypto::aesEncrypt`/`aesDecrypt` nicht implementiert

Kein Bug im engeren Sinn, sondern eine bewusste Lücke: ein XOR- oder
sonstiger Platzhalter unter dem Namen „AES" wäre eine stille
Sicherheitslücke (der Methodenname verspricht echte Verschlüsselung, ein
schwacher Ersatz würde das verdecken). Beide werfen aktuell klar
`"Crypto::aesEncrypt ist nicht implementiert"` statt vorzutäuschen. Bliebe
offen, bis eine echte AES-Implementierung (oder ein bewusster
OpenSSL-Opt-in analog zu HTTPS) gebaut wird.

**Kontext/Historie:** `bugs_fixed.md`, Bug 24.
