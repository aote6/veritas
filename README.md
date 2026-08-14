# Veritas

A Deterministic State Evolution Machine

Veritas is a kernel in which every state change is deterministic, verifiable,
reproducible, and attributable. It is not a JVM competitor, not a blockchain,
and not a database. It is software that can prove how it changed.

## Start here

docs/VERIFICATION_MAP.md                              - What has been verified and frozen. Read this before questioning anything.
STATUS.md                                            - What is implemented now
docs/Veritas_设计文档.md                              - Why is the system shaped this way
docs/Veritas_运行时数据模型标准.md                    - What are the data structures
docs/Veritas_Runtime_Object_规范.md                   - What machine guarantees must Object obey
docs/constitution/                                   - Non-negotiable constraints

## Core commitments

1. Guarantees before performance
2. Explicit over implicit
3. No privileged shortcuts (human or AI)
4. A state root is a claim that must be independently checkable

## Runtime worldview

ModuleObject is a read-only code template.
Runtime Object is the only runtime entity that owns state, participates in
links, and acts as a capability endpoint. Identity at runtime is ObjectId.

## Build and test

cargo test

Progress and capability boundaries live in STATUS.md only, not in this file.
