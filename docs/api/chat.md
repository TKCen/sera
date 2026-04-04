# Chat API

## Send Message

```
POST /api/chat
```

```json
{
  "agentId": "abc-123",
  "message": "Hello, can you help me with a coding task?",
  "sessionId": "optional-existing-session"
}
```

Routes the message to the agent's container chat server. Returns 503 if the container is unavailable.

**Response:** Streamed via SSE or returned as a complete response depending on the `Accept` header.

## List Sessions

```
GET /api/sessions
GET /api/sessions?agentId={id}
```

Returns chat session metadata.

## Get Session History

```
GET /api/sessions/:id/messages
```

Returns the full message history for a session.

## Delete Session

```
DELETE /api/sessions/:id
```

Removes session and associated message history.

## LLM Proxy

```
POST /v1/llm/chat/completions
```

The LLM proxy endpoint used by agent containers. Requires JWT authentication (not API key).

**Request format:** OpenAI-compatible chat completions format.

**Response format:** OpenAI-compatible, with streaming support via SSE.

This endpoint is internal to the SERA network — external clients should use `/api/chat` instead.

## Available Models

```
GET /v1/llm/models
```

Returns available models from all configured providers.

## OpenAI-Compatible Endpoint

```
POST /api/openai/v1/chat/completions
```

An OpenAI-compatible endpoint for external tools that expect the OpenAI API format. Authenticates with `Authorization: Bearer <api-key>`.
