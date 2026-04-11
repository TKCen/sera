# Memory API

## Memory Blocks

### List Memory Blocks

```
GET /api/memory/blocks?agentId={id}&scope={scope}
```

Returns memory blocks for an agent, optionally filtered by scope.

### Get Memory Block

```
GET /api/memory/blocks/:id
```

### Create Memory Block

```
POST /api/memory/blocks
```

```json
{
  "agentId": "abc-123",
  "scope": "personal",
  "category": "observation",
  "title": "User prefers concise responses",
  "content": "The user consistently asks for shorter answers...",
  "tags": ["preferences", "communication"]
}
```

### Delete Memory Block

```
DELETE /api/memory/blocks/:id
```

## Knowledge API

### Store Knowledge

```
POST /api/knowledge/store
```

```json
{
  "agentId": "abc-123",
  "content": "TypeScript 5.4 introduces NoInfer utility type...",
  "scope": "circle",
  "category": "reference",
  "title": "TypeScript 5.4 Features",
  "tags": ["typescript", "language-features"]
}
```

For circle/global scope, this writes to the git-backed knowledge repository.

### Query Knowledge

```
POST /api/knowledge/query
```

```json
{
  "agentId": "abc-123",
  "query": "TypeScript type narrowing patterns",
  "scope": "all",
  "limit": 5,
  "minScore": 0.7
}
```

Returns semantically similar knowledge blocks ranked by relevance.

### Circle Knowledge Management

```
GET /api/knowledge/circles/:id/pending
```

Lists pending merge requests for circle knowledge.

```
POST /api/knowledge/circles/:id/merge
```

Approves and merges pending knowledge from agent branches to the circle's main branch.

## Embeddings

```
POST /api/embedding/generate
```

Generate embeddings for arbitrary text. Used internally by the knowledge pipeline.

```
GET /api/embedding/status
```

Returns the embedding service status (provider, model, dimensions).
