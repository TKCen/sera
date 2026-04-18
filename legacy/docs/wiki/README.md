# SERA LM Wiki

> Persistent knowledge base — architectural decisions, implementation learnings, and patterns.
> Updated as the project evolves. Read by both humans and LLMs for context recovery.

## Index

- [Architecture Overview](architecture.md) — Crate map, dependency graph, MVS scope
- [Config System](config-system.md) — K8s-style manifests, secret resolution, single-file format
- [Turn Loop](turn-loop.md) — Agent reasoning cycle, context assembly, error recovery
- [Storage](storage.md) — SQLite schema, file-based memory, transcript persistence
- [Tools](tools.md) — 7 MVS tools, tool matching, path safety
- [Discord Integration](discord.md) — Gateway protocol, heartbeat, message flow
- [Learnings](learnings.md) — Non-obvious discoveries, gotchas, resolved issues

## Overview

SERA is a **Sandboxed Extensible Reasoning Agent** — a Docker-native AI orchestration platform built in Rust with a 11-crate workspace architecture. The **Minimum Viable System (MVS)** delivers a standalone gateway (`sera` binary) that wires config manifests, SQLite storage, tool execution, and Discord integration into a single zero-dependency process.

### Key Concepts

- **Manifests**: K8s-style YAML declarations (Instance, Provider, Agent, Connector) with support for multi-document files and secret references
- **Turn Loop**: State machine (Init → Think → Act → Observe → Done) with context assembly, LLM calls, tool execution, and error recovery
- **Tools**: 7 built-in functions for memory, file, shell, session management with glob-pattern authorization
- **Storage**: SQLite for sessions and transcripts; file-based markdown for searchable memory
- **Discord**: Native WebSocket gateway protocol for real-time message dispatch

## For LLMs

When implementing features or debugging issues in SERA:

1. **Start with the architecture doc** to understand crate boundaries and dependencies
2. **Check turn-loop.md** to understand reasoning cycle state transitions
3. **Consult learnings.md** for gotchas (e.g., serde_yaml multi-document handling, tokio-tungstenite features)
4. **Validate against tools.md** when adding or modifying tool definitions
5. **Use this wiki as context** for both humans and LLM prompts

---

Last updated: 2026-04-09
