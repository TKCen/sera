# Epic 28: Image Generation

## Overview

Agents need to create images — diagrams, illustrations, visualizations, creative content — as part of their reasoning and output. This epic adds multi-provider image generation as a built-in agent tool, with support for cloud APIs (DALL-E, Stability AI), local models (Stable Diffusion via ComfyUI/A1111), and future providers. Generated images are stored, served via API, and rendered in chat and canvas.

## Context

- **Reference implementation:** OpenClaw `src/image-generation/` — provider registry with runtime selection, model reference resolution, and live test helpers
- **OpenClaw's approach:** Image generation is a registered tool that agents call with a prompt. Provider selected based on config or agent preference. Results returned as tool output (URL or base64).
- **SERA's advantage:** SERA can run local image generation models in isolated containers with GPU passthrough, keeping generation local and private. Budget enforcement (Epic 04) can limit image generation costs per agent.
- **Integration:** Generated images can be pushed to Canvas (Epic 22), sent via channels (Epic 18), or stored as memory artifacts (Epic 08)

## Dependencies

- Epic 04 (LLM Proxy) — budget enforcement for generation API calls
- Epic 05 (Agent Runtime) — tool registration
- Epic 12/13 (sera-web) — image display in chat and agent detail

---

## Stories

### Story 28.1: Image generation service and provider registry

**As** sera-core
**I want** a central image generation service with pluggable providers
**So that** agents can generate images without knowing the underlying provider

**Acceptance Criteria:**

- [ ] `ImageGenerationService` in `core/src/image-generation/ImageGenerationService.ts`
- [ ] `ImageProvider` interface:

  ```typescript
  interface ImageProvider {
    id: string;
    name: string;
    capabilities: {
      sizes: string[]; // e.g. ['256x256', '512x512', '1024x1024']
      styles?: string[]; // e.g. ['natural', 'vivid'] for DALL-E
      models?: string[]; // available model variants
      editing: boolean; // supports image editing/inpainting
      variations: boolean; // supports generating variations
    };
    generate(request: ImageGenRequest): Promise<ImageGenResult>;
    healthCheck(): Promise<boolean>;
  }

  interface ImageGenRequest {
    prompt: string;
    negativePrompt?: string;
    size?: string; // default: '1024x1024'
    style?: string;
    model?: string;
    count?: number; // number of images (default: 1, max: 4)
    quality?: 'standard' | 'hd';
    responseFormat?: 'url' | 'b64_json';
  }

  interface ImageGenResult {
    images: {
      url?: string; // temporary URL (expires)
      b64?: string; // base64-encoded image
      revisedPrompt?: string; // model's interpretation of the prompt
    }[];
    model: string;
    provider: string;
    cost?: number; // estimated cost in USD
  }
  ```

- [ ] Provider registry loaded from `core/config/image-providers.json`
- [ ] Default provider selection: first available provider (operator can set preference)
- [ ] Per-agent provider override via manifest: `spec.imageGeneration.provider: 'dall-e'`

---

### Story 28.2: Cloud image generation providers

**As** an operator
**I want** to use cloud image generation APIs (DALL-E, Stability AI)
**So that** agents can generate high-quality images using cloud services

**Acceptance Criteria:**

- [ ] `DallEProvider` — OpenAI DALL-E 3 / DALL-E 2:
  - Uses existing `OPENAI_API_KEY` from secrets store
  - Supports: generate, variations (DALL-E 2 only)
  - Sizes: 256x256, 512x512, 1024x1024, 1024x1792, 1792x1024
  - Quality: standard, hd
  - Style: natural, vivid
- [ ] `StabilityProvider` — Stability AI (Stable Diffusion 3):
  - Config: `{ apiKey: string }` (stored in secrets)
  - Supports: generate with negative prompts, style presets
  - Sizes: 512x512 to 2048x2048
- [ ] `GoogleImagenProvider` — Google Imagen (via Vertex AI):
  - Uses existing `GOOGLE_API_KEY`
  - Supports: generate
- [ ] Cost tracking: each generation logged to `MeteringService` with estimated cost
- [ ] Rate limiting: configurable max generations per hour per agent (default: 20)

---

### Story 28.3: Local image generation provider

**As** an operator running SERA locally
**I want** to generate images using local Stable Diffusion models
**So that** image generation stays private and free (no API costs)

**Acceptance Criteria:**

- [ ] `ComfyUIProvider` — connects to a ComfyUI instance:
  - Config: `{ baseUrl: string }` (e.g., `http://comfyui:8188`)
  - Sends workflow JSON with prompt parameters
  - Polls for completion, downloads result
  - Supports: txt2img, img2img (with input image)
- [ ] `A1111Provider` — connects to AUTOMATIC1111 / Forge WebUI:
  - Config: `{ baseUrl: string }` (e.g., `http://a1111:7860`)
  - Uses `/sdapi/v1/txt2img` endpoint
  - Supports: txt2img, img2img, negative prompts, samplers, steps
- [ ] `OllamaVisionProvider` — for models that support image generation via Ollama:
  - Uses existing Ollama provider config
- [ ] Docker Compose profile for local SD:
  ```yaml
  sera-image-gen:
    image: ghcr.io/comfyanonymous/comfyui:latest
    profiles: ['image-gen']
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]
    volumes:
      - comfyui_models:/app/models
    networks:
      - sera_net
  ```

---

### Story 28.4: `generate-image` agent tool

**As** an agent
**I want** a built-in tool for generating images
**So that** I can create visual content as part of my work

**Acceptance Criteria:**

- [ ] `generate-image` tool registered in agent tool inventory:
  ```json
  {
    "name": "generate-image",
    "description": "Generate an image from a text description",
    "parameters": {
      "prompt": {
        "type": "string",
        "description": "Detailed description of the image to generate"
      },
      "size": {
        "type": "string",
        "enum": ["256x256", "512x512", "1024x1024"],
        "default": "1024x1024"
      },
      "style": {
        "type": "string",
        "description": "Visual style (e.g., 'photorealistic', 'illustration', 'diagram')"
      },
      "count": { "type": "number", "default": 1, "maximum": 4 }
    }
  }
  ```
- [ ] Tool gated by capability: `tools.allowed` must include `generate-image` (not available by default)
- [ ] Generated images:
  - Stored in `generated_images` table with metadata
  - Served via `GET /api/images/:id` (authenticated)
  - Temporary storage: cleaned up after 7 days (configurable)
- [ ] Tool result returned to agent: `{ imageUrls: ['/api/images/uuid'], revisedPrompt: '...', provider: '...' }`
- [ ] Budget enforcement: generation cost counted against agent's daily token budget (equivalent token cost)

---

### Story 28.5: Image display in sera-web

**As** an operator
**I want** generated images displayed inline in the chat interface
**So that** I can see what agents create without opening separate URLs

**Acceptance Criteria:**

- [ ] Chat messages containing image URLs (`/api/images/:id`) rendered as inline images
- [ ] Image lightbox: click to enlarge with zoom/pan
- [ ] Image actions: download, copy URL, regenerate (sends new generation request)
- [ ] Image gallery: multiple images from a single generation shown as a grid
- [ ] Canvas integration: agents can push generated images to Canvas (Epic 22) via `canvas.push` with `{ type: 'image', src: '/api/images/uuid' }`

---

## DB Schema

```sql
-- Story 28.4: Generated image storage
CREATE TABLE generated_images (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_instance_id uuid REFERENCES agent_instances(id),
  session_id      uuid,
  provider        text NOT NULL,
  model           text,
  prompt          text NOT NULL,
  revised_prompt  text,
  size            text NOT NULL,
  format          text NOT NULL DEFAULT 'png',
  storage_path    text NOT NULL,          -- local file path or S3 key
  file_size       bigint,                 -- bytes
  cost            numeric(10,6),          -- estimated USD cost
  created_at      timestamptz NOT NULL DEFAULT now(),
  expires_at      timestamptz             -- null = never expires
);

CREATE INDEX idx_gen_images_agent ON generated_images(agent_instance_id, created_at DESC);
CREATE INDEX idx_gen_images_expires ON generated_images(expires_at) WHERE expires_at IS NOT NULL;
```

## Configuration

```json
// core/config/image-providers.json
{
  "providers": [
    {
      "id": "dall-e",
      "type": "dall-e",
      "model": "dall-e-3",
      "apiKeySecret": "openai-api-key"
    },
    {
      "id": "comfyui-local",
      "type": "comfyui",
      "baseUrl": "http://sera-image-gen:8188"
    }
  ],
  "defaults": {
    "provider": "dall-e",
    "size": "1024x1024",
    "maxGenerationsPerHour": 20
  }
}
```
