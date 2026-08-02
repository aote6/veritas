# Overview

Superseded as entry by root README.md.
This file remains only as a short pointer; do not record progress here.

Veritas is a deterministic abstract machine that sits above the OS and below
applications. It provides transactions, capabilities, and object lifecycle
semantics that hardware does not natively understand.

## Start here

README.md          - Entry point
STATUS.md          - What is implemented now
设计文档.md         - Why the system is shaped this way
运行时数据模型标准.md - Data structures
Runtime_Object_规范.md - Machine guarantees for Object
constitution/       - Non-negotiable constraints

## Build and test

cargo test
