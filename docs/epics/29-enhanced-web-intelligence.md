# Epic 29: Enhanced Web Intelligence

## Overview

SERA agents currently have basic `web-fetch` and `web-search` skills that return raw content. OpenClaw has a significantly richer web intelligence pipeline — multiple search providers with credential rotation, readability-based link extraction (stripping boilerplate), citation tracking, SSRF-safe fetching, and content-type-aware rendering (HTML → Markdown, JSON formatting). This epic upgrades SERA's web tools to match and exceed that capability.

## Context

- **Reference implementation:** OpenClaw `src/agents/tools/web-search.ts`, `web-fetch.ts`, `web-guarded-fetch.ts`, `src/link-understanding/`, `src/web-search/`
- **OpenClaw's approach:** Web search supports multiple providers (Google, Brave, Tavily, SearXNG) with credential management. Web fetch uses readability extraction (Mozilla Readability) to strip boilerplate. Citation redirect tracking ensures links resolve correctly. SSRF guards prevent internal network access.
- **SERA's current state:** Basic `web-fetch` skill in sera-core with IP filtering. Basic `web-search` skill. Both return raw content without readability processing. No search provider configuration beyond a single default.
- **Integration:** Enhanced web tools benefit all agents across all channels. Results can feed into RAG (Epic 08) and Canvas (Epic 22).

## Dependencies

- Epic 05 (Agent Runtime) — tool registration and agent context
- Epic 04 (LLM Proxy) — budget tracking for search API calls
- Epic 16 (Auth & Secrets) — API key storage for search providers
- Epic 20 (Egress Proxy) — SSRF protection and egress auditing

---

## Stories

### Story 29.1: Multi-provider web search

**As** an agent performing research
**I want** web search results from the best available search provider
**So that** I get high-quality, relevant search results regardless of which provider is configured

**Acceptance Criteria:**

- [ ] `WebSearchService` in `core/src/web/WebSearchService.ts` with provider registry
- [ ] `SearchProvider` interface:

  ```typescript
  interface SearchProvider {
    id: string;
    name: string;
    search(query: string, options?: SearchOptions): Promise<SearchResult[]>;
    healthCheck(): Promise<boolean>;
  }

  interface SearchOptions {
    maxResults?: number; // default: 10
    language?: string; // ISO 639-1
    region?: string; // ISO 3166-1
    timeRange?: 'day' | 'week' | 'month' | 'year';
    safeSearch?: boolean; // default: true
  }

  interface SearchResult {
    title: string;
    url: string;
    snippet: string;
    source: string; // provider that returned this result
    publishedDate?: string;
    score?: number; // relevance score if provider supplies it
  }
  ```

- [ ] **Built-in providers:**
  - `SearXNGProvider` — self-hosted SearXNG instance (local, free, private)
    - Config: `{ baseUrl: string }` (e.g., `http://sera-searxng:8080`)
    - Docker Compose profile: `searxng` with `searxng/searxng:latest` image
  - `BraveSearchProvider` — Brave Search API
    - Config: `{ apiKeySecret: string }`
    - Free tier: 2000 queries/month
  - `TavilyProvider` — Tavily Search API (AI-optimized search)
    - Config: `{ apiKeySecret: string }`
    - Returns AI-extracted answers alongside results
  - `GoogleSearchProvider` — Google Custom Search API
    - Config: `{ apiKeySecret: string, searchEngineId: string }`
  - `DuckDuckGoProvider` — DDG Instant Answer API (no API key needed)
    - Limited to instant answers, not full search
- [ ] Provider selection: first healthy provider in configured priority order
- [ ] Fallback: if primary provider fails, try next in list
- [ ] Result deduplication: if multiple providers return same URL, keep highest-ranked
- [ ] Configuration in `core/config/search-providers.json`
- [ ] `web-search` tool updated to use `WebSearchService` instead of current hardcoded implementation

---

### Story 29.2: Link understanding and readability extraction

**As** an agent fetching a web page
**I want** the page content extracted cleanly (article text, no boilerplate)
**So that** I get useful content without navigation, ads, footers, and cookie banners

**Acceptance Criteria:**

- [ ] `LinkUnderstandingService` in `core/src/web/LinkUnderstandingService.ts`
- [ ] **Readability pipeline:**
  1. Fetch URL with safe HTTP client (Story 29.4)
  2. Detect content type from headers
  3. For `text/html`: apply Mozilla Readability (`@mozilla/readability` + `jsdom`) to extract article content
  4. Convert extracted HTML to Markdown (`turndown` library)
  5. For `application/json`: pretty-print with truncation
  6. For `text/plain`: return as-is with truncation
  7. For `application/pdf`: route to Media Processing Pipeline (Epic 25) for extraction
  8. For images: route to image analysis (Epic 25)
- [ ] **Output:**
  ```typescript
  interface LinkContent {
    url: string;
    finalUrl: string; // after redirects
    title: string;
    byline?: string; // author
    content: string; // Markdown text
    excerpt?: string; // first ~200 chars
    contentType: string;
    wordCount: number;
    language?: string;
    publishedDate?: string;
    siteName?: string;
  }
  ```
- [ ] Content truncation: configurable max output length (default: 8000 tokens, ~32KB)
- [ ] Redirect following: up to 5 redirects, tracking final URL
- [ ] JavaScript rendering: optional Playwright-based rendering for JS-heavy sites (Story 29.5)
- [ ] Cache: extracted content cached for 1 hour (configurable) to avoid re-fetching
- [ ] `web-fetch` tool updated to use `LinkUnderstandingService`

---

### Story 29.3: Citation tracking

**As** an agent citing web sources in its response
**I want** citations properly tracked with source URLs
**So that** operators can verify information sources and agents maintain attribution

**Acceptance Criteria:**

- [ ] `CitationService` in `core/src/web/CitationService.ts`
- [ ] When an agent uses `web-search` or `web-fetch`, the source URL is registered as a citation:
  ```typescript
  interface Citation {
    id: string;
    sessionId: string;
    agentInstanceId: string;
    url: string;
    title: string;
    accessedAt: string;
    snippet?: string; // relevant excerpt used
  }
  ```
- [ ] Citations stored in `citations` table, linked to chat session
- [ ] Agent context includes citation instructions: "When using information from web sources, cite them using [n] notation"
- [ ] `GET /api/sessions/:id/citations` returns all citations for a session
- [ ] sera-web: citations rendered as footnote links in chat messages
  - `[1]` in message text links to citation URL
  - Citation sidebar/footer shows full source list
- [ ] Citation redirect resolution: follow URL redirects to get canonical URL (avoid tracking redirects)

---

### Story 29.4: SSRF-safe HTTP client

**As** sera-core
**I want** all outbound HTTP requests from web tools to be SSRF-protected
**So that** agents cannot use web-fetch to access internal services or private networks

**Acceptance Criteria:**

- [ ] `SafeHttpClient` in `core/src/web/SafeHttpClient.ts` — wraps `fetch` with security guards
- [ ] **IP blocking:**
  - Block private ranges: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8
  - Block link-local: 169.254.0.0/16
  - Block IPv6 private ranges
  - DNS resolution before connection to prevent DNS rebinding
- [ ] **Protocol restrictions:** Only `http:` and `https:` (block `file:`, `ftp:`, `data:`, `javascript:`)
- [ ] **Domain blocking:** Configurable denylist (default: block `metadata.google.internal`, `169.254.169.254`, internal Docker hostnames)
- [ ] **Timeouts:** Connect timeout 10s, response timeout 30s (configurable)
- [ ] **Size limits:** Max response body 10MB (configurable)
- [ ] **User-Agent:** `SERA-Agent/1.0 (+https://github.com/TKCen/sera)` (identifiable, not spoofing browsers)
- [ ] All web tool HTTP requests routed through `SafeHttpClient`
- [ ] Integration with egress proxy (Epic 20): if proxy configured, route through Squid for additional ACL enforcement

---

### Story 29.5: JavaScript-rendered page fetching

**As** an agent fetching a modern web application
**I want** pages with JavaScript-rendered content properly extracted
**So that** I can read SPAs and dynamic sites that don't work with basic HTTP fetch

**Acceptance Criteria:**

- [ ] `BrowserFetchProvider` — uses Playwright to render pages before extraction
- [ ] Runs in a dedicated container (`sera-browser-worker`) with Playwright + Chromium:
  ```yaml
  sera-browser-worker:
    image: mcr.microsoft.com/playwright:v1.50.0-jammy
    profiles: ['browser']
    networks:
      - sera_net
    deploy:
      resources:
        limits:
          memory: 1G
          cpus: '1'
  ```
- [ ] Render pipeline:
  1. Launch headless Chromium page
  2. Navigate to URL with 15s timeout
  3. Wait for network idle (no pending requests for 2s)
  4. Extract rendered HTML
  5. Apply readability pipeline (Story 29.2)
  6. Close page
- [ ] Fallback: if browser rendering unavailable or times out, fall back to basic HTTP fetch
- [ ] `web-fetch` tool gains `render: true` option to explicitly request JS rendering
- [ ] Auto-detection: if basic fetch returns minimal content (< 500 chars) from a known SPA domain, auto-retry with browser rendering
- [ ] Concurrency: max 3 concurrent browser pages (configurable)
- [ ] Security: browser runs with `--no-sandbox` in container but network restricted to `sera_net`

---

### Story 29.6: Search and fetch observability

**As** an operator
**I want** to see what web searches and fetches agents are performing
**So that** I can audit web access and optimize search provider usage

**Acceptance Criteria:**

- [ ] `GET /api/web/search-history` — recent search queries with results count, provider used, latency
- [ ] `GET /api/web/fetch-history` — recent URL fetches with status, content type, size, latency
- [ ] Both endpoints support filtering by agent, time range, and status
- [ ] Web activity logged to audit trail: `web.search`, `web.fetch` events with metadata
- [ ] Search provider usage dashboard in sera-web (if multiple providers configured):
  - Queries per provider per day
  - Average latency per provider
  - Error rate per provider
  - Remaining quota (for providers with rate limits)

---

## DB Schema

```sql
-- Story 29.3: Citation tracking
CREATE TABLE citations (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  session_id      uuid NOT NULL,
  agent_instance_id uuid REFERENCES agent_instances(id),
  url             text NOT NULL,
  final_url       text,                   -- after redirect resolution
  title           text,
  snippet         text,
  accessed_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_citations_session ON citations(session_id, accessed_at);

-- Story 29.6: Web activity log (lightweight, for dashboard)
CREATE TABLE web_activity_log (
  id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_instance_id uuid REFERENCES agent_instances(id),
  activity_type   text NOT NULL,          -- 'search' | 'fetch'
  query_or_url    text NOT NULL,
  provider        text,
  status          text NOT NULL,          -- 'success' | 'error'
  result_count    int,                    -- for search
  content_type    text,                   -- for fetch
  response_size   int,                    -- bytes
  latency_ms      int,
  error           text,
  created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_web_activity_agent ON web_activity_log(agent_instance_id, created_at DESC);
CREATE INDEX idx_web_activity_type ON web_activity_log(activity_type, created_at DESC);
```

## Configuration

```json
// core/config/search-providers.json
{
  "providers": [
    {
      "id": "searxng-local",
      "type": "searxng",
      "baseUrl": "http://sera-searxng:8080",
      "priority": 1
    },
    {
      "id": "brave",
      "type": "brave",
      "apiKeySecret": "brave-search-api-key",
      "priority": 2
    }
  ],
  "defaults": {
    "maxResults": 10,
    "safeSearch": true,
    "cacheMinutes": 60
  },
  "fetch": {
    "maxResponseSize": "10MB",
    "readabilityEnabled": true,
    "maxOutputTokens": 8000,
    "browserRenderingEnabled": false
  }
}
```

## Docker Compose additions

```yaml
# Self-hosted search (Story 29.1)
sera-searxng:
  image: searxng/searxng:latest
  profiles: ['searxng']
  volumes:
    - searxng_data:/etc/searxng
  networks:
    - sera_net
  environment:
    - SEARXNG_BASE_URL=http://sera-searxng:8080

# Browser rendering (Story 29.5)
sera-browser-worker:
  image: mcr.microsoft.com/playwright:v1.50.0-jammy
  profiles: ['browser']
  networks:
    - sera_net
  deploy:
    resources:
      limits:
        memory: 1G
        cpus: '1'
```
