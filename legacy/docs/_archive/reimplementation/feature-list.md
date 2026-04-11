# 📋 Feature List: The "Serified" OpenFang

This document catalogs the features ported from OpenFang and their modernized counterparts in the SERA architecture.

## 🛡️ Core Security Features
*   **Sandholed Execution (Tiered)**: Replaces OpenFang's WASMSbox with Docker-native cgroup isolation. 
    *   *Tier 1 (Read Only)*: No network, restricted filesystem.
    *   *Tier 2 (Internal)*: Local network access, restricted filesystem.
    *   *Tier 3 (Executive)*: Full internet access, ephemeral persistence.
*   **Merkle Audit Trail**: All actions cryptographically linked in a PostgreSQL event store for tamper-proof history.
*   **Information Flow Tagging**: Modernized "Taint Tracking" using LLM-native context metadata to prevent secret leakage.
*   **Capability-Based RBAC**: Fine-grained tool access controlled via Ed25519-signed agent manifests.

## 🤖 The Hands (WorkerAgents)
OpenFang's autonomous "Hands" are reimplemented as specialized **WorkerAgents** within SERA.

| Hand | Serified Capability | Status |
| :--- | :--- | :--- |
| **Researcher** | Multi-hop autonomous research with CRAAP credibility scoring. | 📅 Planned |
| **Collector** | Continuous intelligence monitoring + Knowledge Graph (Qdrant). | 📅 Planned |
| **Browser** | Playwright-based web automation with human-in-the-loop approval. | 📅 Planned |
| **Clip** | Multi-phase media processing pipeline (FFmpeg + yt-dlp). | ⏳ Backlog |
| **Lead** | Daily lead generation and enrichment with ICP profiling. | ⏳ Backlog |
| **Predictor** | Superforecasting engine with Brier-score tracking. | ⏳ Backlog |
| **Twitter** | Autonomous content lifecycle management for X/Twitter. | ⏳ Backlog |

## 🔗 Connectivity & Adapters
*   **Multi-Channel Core**: Native support for **Telegram, Discord, and Slack**.
*   **WhatsApp Web Gateway**: QR-code based connection (No Meta Business API required).
*   **Adapter Registry**: Extensible framework for adding 40+ supported messaging platforms.
*   **MCP Support**: Native integration with the **Model Context Protocol** for universal tool access.

## 🧠 Memory & Knowledge
*   **Tiered Memory System**: 
    1.  *Working*: Context-aware ephemeral state.
    2.  *Archival*: Sequential Markdown storage for long-term recall.
    3.  *Semantic*: Vectorized knowledge base (Qdrant) for "Deep Recall".
*   **LSP-Native Ingestion**: Contextual understanding of codebases using the Language Server Protocol.
*   **Automatic Compaction**: Periodic summarization of old "Working Memory" into "Archival" stores.

## 🎨 UI & Ecosystem (The Experience)
*   **AIU Aurora Cyber Dashboard**: Next.js 16 + Tailwind v4 interface perfectly aligned with the `radio-player` blueprint.
*   **Themed Glassmorphism**: Semi-transparent "Aurora Black" panels with 10-20% opacity green/cyan borders.
*   **Thought Streaming**: Real-time visualization of agent reasoning using the brand gradient (Cyan to Green).
*   **Unified TUI/CLI**: Modern daemon management using matching terminal color schemes.
*   **Homelab Integration**: Native discovery via Homepage and monitoring via Uptime Kuma.
