# Epic 23: Voice Interface

## Overview

Low-latency voice interaction for ambient agent access. Operators can talk to SERA agents hands-free — voice input is transcribed, routed to an agent, and the response is spoken back. This enables use cases where keyboard interaction is impractical: standing meetings, home automation oversight, mobile agent check-ins, and accessibility.

The voice interface is delivered in two phases: **Phase 4** (browser-based, Web Speech API) and **Phase 5** (companion apps with wake words and continuous listening — deferred).

## Context

- Phase 4 uses browser-native Web Speech API for STT and either browser TTS or ElevenLabs for speech synthesis
- Voice is a channel (Epic 18) — voice input becomes ingress events, voice output is egress
- Agent thinking and tool calls can optionally be narrated (configurable verbosity)
- Push-to-talk (PTT) is the default mode — continuous listening deferred to Phase 5
- Reference: OpenClaw voice implementation (macOS/iOS/Android menu bar app, talk mode, camera/screen recording)

## Dependencies

- Epic 09 (Real-Time Messaging) — Centrifugo for streaming agent responses to voice TTS
- Epic 12/13 (Dashboard) — sera-web hosting the voice controls
- Epic 18 (Integration Channels) — voice as a channel type

---

## Stories

### Story 23.1: Speech-to-text input

**As** an operator
**I want** to speak to SERA agents using my microphone
**So that** I can interact hands-free without typing

**Acceptance Criteria:**
- [ ] `VoiceInput` React component — push-to-talk button with visual feedback (recording indicator)
- [ ] Uses Web Speech API (`SpeechRecognition`) for real-time transcription
- [ ] Interim transcription shown in chat input field as user speaks
- [ ] Final transcript submitted as a chat message on PTT release
- [ ] Language configurable (defaults to browser locale)
- [ ] Microphone permission requested with clear UX explanation
- [ ] Graceful fallback if Web Speech API unavailable (button disabled with tooltip)

### Story 23.2: Text-to-speech output

**As** an operator
**I want** agent responses spoken aloud
**So that** I can hear answers without reading the screen

**Acceptance Criteria:**
- [ ] `VoiceOutput` service — converts agent response text to speech
- [ ] Default: browser `SpeechSynthesis` API (zero cost, works offline)
- [ ] Optional: ElevenLabs API integration for higher-quality voices
  - Configured via Settings page: API key, voice ID, model
  - Falls back to browser TTS if ElevenLabs unavailable
- [ ] TTS streams chunk-by-chunk as agent response arrives (low latency)
- [ ] Speaking indicator shown in chat UI (audio wave animation)
- [ ] Operator can interrupt (click or new PTT) to stop current TTS

### Story 23.3: Voice routing and agent selection

**As** an operator using voice
**I want** to direct my voice message to a specific agent
**So that** I get the right expertise without navigating the UI

**Acceptance Criteria:**
- [ ] Voice routing uses the currently-selected agent in chat view
- [ ] Agent switch via voice command: "talk to [agent name]" detected as routing command
- [ ] Confirmation feedback: "[Agent name] is listening" spoken before routing
- [ ] If no agent selected, routes to default agent or circle

### Story 23.4: Voice mode settings

**As** an operator
**I want** to configure voice behavior
**So that** the voice interface matches my preferences and environment

**Acceptance Criteria:**
- [ ] Voice settings panel in Settings page:
  - TTS provider: browser / ElevenLabs
  - TTS voice selection (from available voices)
  - Speech rate and pitch
  - Narrate thinking: on / summary / off
  - Narrate tool calls: on / off
  - PTT key binding (default: Space when voice panel focused)
- [ ] Settings persisted in `operator_preferences` table
- [ ] Settings applied immediately (no page reload)

### Story 23.5: Voice channel integration

**As** sera-core
**I want** voice interactions logged as channel events
**So that** voice conversations appear in audit trail and chat history

**Acceptance Criteria:**
- [ ] Voice input creates ingress events on the `voice` channel type
- [ ] Voice output creates egress events with TTS metadata (provider, voice, duration)
- [ ] Chat history shows voice messages with a microphone icon indicator
- [ ] Audit trail includes voice event metadata (transcription confidence, audio duration)

---

## DB Schema

```sql
-- Story 23.4: Voice preferences per operator
-- Uses existing operator_preferences table with JSON key:
-- key: 'voice_settings'
-- value: { ttsProvider, voiceId, speechRate, pitch, narrateThinking, narrateToolCalls, pttKey }

-- Story 23.5: Voice event metadata (extends existing audit_events)
-- No new table — voice metadata stored in audit_events.metadata jsonb field:
-- { channel: 'voice', transcriptionConfidence: 0.95, audioDurationMs: 3200, ttsProvider: 'browser' }
```
