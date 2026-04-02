# Epic 25: Media Processing Pipeline

## Overview

Agents need to understand multimodal inputs — images, audio, video, and PDFs — to be genuinely useful assistants. Currently SERA agents can only process text. This epic adds a media processing pipeline that transcribes audio, extracts text from PDFs, describes images, and summarises video content. All processing happens server-side (in sera-core or dedicated containers) so agents across all channels benefit equally.

## Context

- **Reference implementation:** OpenClaw `src/media-understanding/` — provider-based audio transcription (Deepgram, OpenAI Whisper), image analysis (vision models), video frame extraction, PDF text extraction
- **OpenClaw's approach:** In-process media processing with provider registry, attachment normalization, audio preflight checks, and configurable concurrency limits
- **SERA's advantage:** Container isolation means media processing can run in dedicated sidecar containers with resource limits, preventing a large video from starving agent containers
- **Integration points:** Channel adapters (Epic 18) receive media attachments → media pipeline processes them → extracted content injected into agent context or stored as memory blocks

## Dependencies

- Epic 04 (LLM Proxy) — vision-capable model routing for image/video analysis
- Epic 05 (Agent Runtime) — tool registration for media tools
- Epic 08 (Memory & RAG) — storing extracted media content as memory blocks
- Epic 18 (Integration Channels) — receiving media attachments from Discord/Slack/Telegram

---

## Stories

### Story 25.1: Media processing service and provider registry

**As** sera-core
**I want** a central media processing service with pluggable providers
**So that** any agent or channel adapter can request media processing without knowing the underlying provider

**Acceptance Criteria:**

- [ ] `MediaService` in `core/src/media/MediaService.ts` — orchestrates media processing requests
- [ ] `MediaProvider` interface:

  ```typescript
  interface MediaProvider {
    id: string;
    capabilities: (
      | 'audio-transcription'
      | 'image-analysis'
      | 'video-analysis'
      | 'pdf-extraction'
    )[];
    process(input: MediaInput): Promise<MediaOutput>;
    healthCheck(): Promise<boolean>;
  }

  interface MediaInput {
    type: 'audio' | 'image' | 'video' | 'pdf';
    source: Buffer | string; // buffer or URL/path
    mimeType: string;
    options?: {
      language?: string; // for transcription
      maxPages?: number; // for PDF
      frameInterval?: number; // for video (seconds between extracted frames)
      prompt?: string; // hint for image/video analysis
    };
  }

  interface MediaOutput {
    type: string;
    text: string; // extracted/transcribed/described text
    metadata: {
      duration?: number; // audio/video duration in seconds
      pages?: number; // PDF page count
      confidence?: number; // transcription confidence 0-1
      tokens?: number; // estimated token count of output
    };
  }
  ```

- [ ] Provider registry loads from `core/config/media-providers.json`
- [ ] Capability-based routing: `MediaService.process(input)` selects best available provider for the input type
- [ ] Concurrency limiting: configurable max concurrent processing jobs (default: 3)
- [ ] File size limits: configurable per media type (default: audio 25MB, image 10MB, video 100MB, PDF 50MB)
- [ ] Processing timeout: configurable (default: 120s for audio/video, 30s for image/PDF)

---

### Story 25.2: Audio transcription

**As** an agent receiving a voice message
**I want** the audio automatically transcribed to text
**So that** I can understand and respond to voice inputs from any channel

**Acceptance Criteria:**

- [ ] `WhisperProvider` — uses OpenAI-compatible Whisper API (works with local Whisper servers and OpenAI API)
  - Config: `{ baseUrl, apiKey?, model: 'whisper-1' }`
  - Supports: mp3, mp4, mpeg, mpga, m4a, wav, webm
- [ ] `DeepgramProvider` — uses Deepgram API for real-time and batch transcription
  - Config: `{ apiKey, model: 'nova-2', language?: string }`
- [ ] Audio preflight checks:
  - File size within limits
  - Duration check (reject > 2 hours)
  - Format validation (magic bytes)
  - Skip tiny files (< 1s duration) — likely accidental recordings
- [ ] Transcription result includes: text, language detected, confidence, duration, word-level timestamps (if available)
- [ ] `transcribe-audio` agent tool: agents can explicitly request transcription of an audio URL/attachment
- [ ] Channel integration: audio attachments from Discord/Telegram/WhatsApp auto-transcribed and injected as user message context

---

### Story 25.3: Image analysis

**As** an agent receiving an image
**I want** the image analysed and described
**So that** I can understand visual content shared by operators

**Acceptance Criteria:**

- [ ] `VisionModelProvider` — routes image to a vision-capable LLM (GPT-4o, Claude, Gemini, local LLaVA)
  - Uses existing `LlmRouter` with `input: ['image']` capability filter
  - Prompt: configurable system prompt for image analysis (default: "Describe this image in detail")
- [ ] Image preprocessing:
  - Resize large images to max 2048px on longest side (configurable)
  - Convert HEIC/HEIF to JPEG for provider compatibility
  - Strip EXIF data (privacy)
- [ ] `analyze-image` agent tool: agents can request analysis of an image URL/attachment with a custom prompt
- [ ] Result: description text, detected objects/text (if available), image dimensions
- [ ] Channel integration: image attachments from channels auto-described and injected as context

---

### Story 25.4: PDF text extraction

**As** an agent receiving a PDF document
**I want** the text content extracted
**So that** I can read and reason about document contents

**Acceptance Criteria:**

- [ ] `PdfExtractProvider` — extracts text from PDF using `pdf-parse` (lightweight, no native deps)
  - Fallback: `pdfjs-dist` for PDFs with complex layouts
- [ ] `VisionPdfProvider` — for scanned/image PDFs: renders pages to images, then uses vision model OCR
  - Only activated when text extraction yields < 100 chars per page (likely scanned)
- [ ] Page range support: extract specific pages (e.g., `pages: '1-5'`)
- [ ] Output: extracted text per page, total page count, whether OCR was used
- [ ] `read-pdf` agent tool: agents can request PDF extraction with optional page range
- [ ] Large PDF handling: chunk extraction for PDFs > 50 pages, return summary + page index
- [ ] Channel integration: PDF attachments auto-extracted and injected as context

---

### Story 25.5: Video understanding

**As** an agent receiving a video
**I want** the video's content summarised (audio transcription + key frame analysis)
**So that** I can understand video content without watching it

**Acceptance Criteria:**

- [ ] Video processing pipeline:
  1. Extract audio track → transcribe via Story 25.2
  2. Extract key frames at configurable interval (default: every 30s) using `ffmpeg`
  3. Analyse key frames via Story 25.3 (batch, max 10 frames)
  4. Combine: audio transcript + frame descriptions → summary
- [ ] `ffmpeg` runs in a sidecar container (`sera-media-worker`) — not in sera-core process
  - Docker image: `jrottenberg/ffmpeg:slim` or custom minimal image
  - Added to docker-compose with resource limits (CPU: 1, Memory: 512MB)
- [ ] `analyze-video` agent tool: agents can request video analysis
- [ ] Output: transcript, key frame descriptions, combined summary, duration, resolution
- [ ] Duration limit: reject videos > 30 minutes (configurable)
- [ ] Channel integration: video attachments from channels trigger pipeline, summary injected as context

---

### Story 25.6: Media attachment handling in channels

**As** sera-core
**I want** a unified media attachment pipeline for all channels
**So that** media received via Discord, Telegram, Slack, or webhooks is processed consistently

**Acceptance Criteria:**

- [ ] `MediaAttachment` type added to `IngressEvent` (Epic 18):
  ```typescript
  interface MediaAttachment {
    id: string;
    type: 'audio' | 'image' | 'video' | 'pdf' | 'document';
    mimeType: string;
    filename?: string;
    size: number;
    url?: string; // platform CDN URL (ephemeral)
    localPath?: string; // after download to temp storage
  }
  ```
- [ ] `IngressRouter` detects media attachments and:
  1. Downloads to temp storage (configurable: local disk or S3-compatible)
  2. Passes through `MediaService.process()` for content extraction
  3. Injects extracted text as `<media-context>` block in agent message
  4. Cleans up temp files after processing (configurable retention: 1 hour default)
- [ ] Per-agent media processing config in manifest:
  ```yaml
  spec:
    media:
      enabled: true
      autoProcess: ['audio', 'image', 'pdf'] # auto-process these types
      maxFileSize: 25MB
  ```
- [ ] Agents without `media.enabled` receive the raw attachment reference but no processed content
- [ ] Media processing metered: token cost of vision/transcription calls tracked in `MeteringService`

---

### Story 25.7: Media processing observability

**As** an operator
**I want** to monitor media processing activity and costs
**So that** I can track usage and debug processing failures

**Acceptance Criteria:**

- [ ] `GET /api/media/stats` — processing counts by type, success/failure rates, average processing time, total tokens consumed
- [ ] `GET /api/media/history` — recent processing jobs with status, input type, provider used, duration, output size
- [ ] Media processing events logged to audit trail: `media.processed`, `media.failed` with metadata
- [ ] Dashboard widget in sera-web (Epic 14) showing media processing stats
- [ ] Failed processing: error details logged, agent receives fallback message ("Unable to process attached [type]")

---

## DB Schema

```sql
-- Story 25.1: Media processing job tracking
CREATE TABLE media_processing_jobs (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_instance_id uuid REFERENCES agent_instances(id),
  session_id      uuid,
  input_type      text NOT NULL,          -- 'audio' | 'image' | 'video' | 'pdf'
  input_mime_type text NOT NULL,
  input_size      bigint NOT NULL,        -- bytes
  provider_id     text NOT NULL,
  status          text NOT NULL DEFAULT 'pending',  -- 'pending' | 'processing' | 'completed' | 'failed'
  output_text     text,
  output_tokens   int,
  metadata        jsonb DEFAULT '{}',     -- provider-specific metadata
  error           text,
  started_at      timestamptz,
  completed_at    timestamptz,
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_media_jobs_agent ON media_processing_jobs(agent_instance_id, created_at DESC);
CREATE INDEX idx_media_jobs_status ON media_processing_jobs(status);
```

## Configuration

```json
// core/config/media-providers.json
{
  "providers": [
    {
      "id": "whisper-local",
      "type": "whisper",
      "capabilities": ["audio-transcription"],
      "baseUrl": "http://localhost:8080/v1",
      "model": "whisper-1"
    },
    {
      "id": "vision-default",
      "type": "vision-model",
      "capabilities": ["image-analysis", "video-analysis"],
      "model": "gpt-4o"
    },
    {
      "id": "pdf-local",
      "type": "pdf-extract",
      "capabilities": ["pdf-extraction"]
    }
  ],
  "limits": {
    "maxConcurrent": 3,
    "audio": { "maxSize": "25MB", "maxDuration": 7200, "timeout": 120000 },
    "image": { "maxSize": "10MB", "timeout": 30000 },
    "video": { "maxSize": "100MB", "maxDuration": 1800, "timeout": 300000 },
    "pdf": { "maxSize": "50MB", "maxPages": 200, "timeout": 60000 }
  }
}
```
