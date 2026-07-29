# Veritas

An abstract machine that defines new physics for software: what entities are, how they are isolated, and how they can safely cooperate without trusting each other.


## What This Is

Linux solved one problem: how multiple programs share one computer.

Veritas solves a different problem: how multiple entities cooperate in the same world while being physically incapable of accessing each other's data.

Not through encryption. Not through permission flags. Not through trust. Through the machine's own physical laws.


## Why This Exists

Operating systems isolate processes. But they cannot isolate components inside a process. Once a process has permission to read a file, every module inside that process can read it. The ad SDK, the analytics module, the chat module—all share the same access.

Veritas isolates at the entity level. You define what an entity is. The machine guarantees that entity A cannot read entity B's state unless B explicitly grants a capability. Same process, same address space, same hardware. Physically impossible to violate.


## What It Does Today

OBJECT_BIRTH  - Create an entity. Unique identity. Zero-trace on abort. Survives crash recovery.
OBJECT_LINK   - Connect two entities. Self-loop rejected. Topology persisted to WAL.
READ / WRITE  - Read and write barriers with automatic version tracking.
COMMIT / ABORT - All-or-nothing transactions. Abort leaves zero trace.
SAVEPOINT     - Nested rollback. Entities and links roll back correctly.
EFFECT        - Deferred side effects. Automatically retried after crash.
WAL           - Write-ahead log. World restores to consistent state after crash.

56 tests. All passing. Each test runs in its own isolated WAL environment.


## What It Is Not

Not a database. Not a blockchain. Not a programming language. Not an end-user product.

It is a layer of physical laws. Others can build systems on top of it—the same way Android, Ubuntu, and router firmware are built on top of Linux.


## Quick Start

git clone https://github.com/aote6/veritas.git
cd veritas
cargo test


## Status

v0.1.0 — Transaction kernel + Scope + WAL + Savepoint + Object + Link + Recovery.

See STATUS.md for details.


## License

GPL-3.0
