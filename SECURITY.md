# Security Policy

## Supported branch

Security fixes should target the `main` branch.

## Current security posture

Nexum Graph includes the following baseline protections:

- Remote `nex serve` binds require auth unless explicitly overridden.
- Repo-local auth config stores token hashes at rest.
- Raw bearer tokens are only emitted once at issue time.
- Server audit records are hash chained and anchored by a separate head file.
- `nex audit verify` detects local audit corruption and truncation.
- Atomic writes with backup recovery protect critical `.nex/` state files.

## What is security-sensitive in this repo

Please treat the following areas as security-sensitive:

- `nex serve` auth and authorization logic
- audit log integrity
- rollback and replay integrity
- persistence and recovery code under `.nex/`
- remote exposure and network defaults
- token issuance, revocation, and migration paths

## Reporting a vulnerability

Do not open a public issue for an undisclosed security vulnerability.

Until a dedicated security contact is published in the repository settings, report privately to the maintainers through the contact method listed on the repository or organization profile. If private reporting is not available, request a private channel before sharing details.

Please include:

- affected commit or branch
- reproduction steps
- impact assessment
- whether the issue affects local-only deployments, remote server deployments, or both
- any suggested mitigation

## Operational guidance

If you are running `nex serve` beyond localhost:

- initialize auth before binding to a non-loopback interface
- rotate tokens with `nex auth issue` and `nex auth revoke`
- run `nex audit verify` after operational incidents or suspicious host activity
- keep `.nex/` out of source control
- treat local host access as trusted, because local tamper resistance is not the same as remote transparency

## Known limits

Current protections are strong for local integrity, but not yet externally anchored. A sufficiently privileged local attacker could still alter both the audit log and its head anchor. Remote or signed transparency anchoring is an active hardening area.
