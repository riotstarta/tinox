# Security Policy

Tinox is a young, actively developed language implementation (compiler,
runtime, and standard library). It has not undergone an independent
security audit and should not yet be treated as hardened for running
untrusted code or handling untrusted network input in production.

## Reporting a Vulnerability

If you find a security issue — for example a memory-safety bug in the
runtime, a compiler bug that produces unsafe generated code, or a flaw in
a stdlib module such as `crypto`, `jwt`, `amqp091`/`amqp10` (TLS handling),
`http_server`, or `websocket` — please report it privately rather than
opening a public issue:

- Preferred: use [GitHub's private vulnerability reporting](https://github.com/subnix-work/tinox/security/advisories/new)
  for this repository.
- If that's not available to you, open a regular issue that asks for a
  private contact channel without including exploit details, and we'll
  follow up.

Please include:

- A description of the issue and its impact.
- Steps to reproduce (a minimal `.tnx` program, if applicable).
- Which component is affected (compiler/typechecker, codegen, C runtime,
  or a specific `tinox-core` module).

We'll acknowledge reports as soon as we can and work with you on a fix
and disclosure timeline. Once a fix is available, we'll publish it via a
GitHub issue/advisory following the project's normal bug-tracking
conventions.

## Scope notes

Some stdlib modules explicitly document known, non-hardened gaps in their
v1 implementation (e.g. no publisher confirms in `amqp091`, no
fragmentation support in `websocket`, self-signed-certificate opt-outs in
TLS clients via `verify=false`). These are tracked as design limitations
in [GitHub issues](https://github.com/subnix-work/tinox/issues), not
silent vulnerabilities — but reports pointing out cases where a
documented limitation is worse than expected (e.g. exploitable, not just
incomplete) are still very welcome.
