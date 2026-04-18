# Task File Template

Pass a JSON task file to a lane session to define its work contract.

```json
{
  "taskId": "sera-<bead-id>",
  "lane": "omc|omx|gemini",
  "cwd": "~/projects/sera",
  "beadId": "<id>",
  "goal": "<one sentence>",
  "inputs": [],
  "writeArtifacts": [
    "artifacts/handoffs/<session-name>.md"
  ],
  "stopCondition": "<explicit condition>",
  "verify": [],
  "resumeFrom": "artifacts/handoffs/<session-name>.md"
}
```

## Field reference

| Field | Description |
|---|---|
| `taskId` | Unique ID matching bead ID |
| `lane` | Which lane this task is routed to |
| `cwd` | Working directory — always `~/projects/sera` |
| `beadId` | The bead (issue) ID this task belongs to |
| `goal` | One-sentence goal — what done looks like |
| `inputs` | Files/artifacts the lane should read before starting |
| `writeArtifacts` | Where the lane must write its output handoff |
| `stopCondition` | Explicit, checkable condition for the lane to stop |
| `verify` | Steps to run to confirm the work is correct |
| `resumeFrom` | Handoff file to read if resuming a prior session |
