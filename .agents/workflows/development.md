---
description: how to initialize and develop Project SERA
---
# SERA Development Workflow

This workflow guides Antigravity (or any collaborator) through the standard development cycle for Project SERA.

## 🚀 Steps

### 1. Planning & Design
- Review [project_documentation.md](file:///C:/Users/TKC/.gemini/antigravity/brain/e1f70a86-151c-4023-ac77-760e2972ffa1/project_documentation.md) for architectural context.
- Update the implementation plan for the current phase.
- Ensure any UI changes align with the [Design Guide](file:///C:/Users/TKC/.gemini/antigravity/brain/e1f70a86-151c-4023-ac77-760e2972ffa1/design_guide.md).

### 2. Environment Setup
// turbo
- Ensure the Docker environment is healthy:
  ```powershell
  docker compose ps
  ```

### 3. Implementation
- Work in small, verifiable increments.
- Maintain the decoupled architecture: logic in `core/`, UI in `web/`.
- Use the sandbox for all execution and testing.

### 4. Verification
- Run builds and tests:
  ```powershell
  docker compose build
  bun run test # in respective service directories
  ```
- Verify real-time updates via Centrifugo.

### 5. Documentation
- Update `task.md` periodically.
- Log significant changes in the `walkthrough.md` for user review.
