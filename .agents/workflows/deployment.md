---
description: how to deploy the SERA multi-container stack
---
# SERA Deployment Workflow

Steps to deploy the full SERA orchestrator and dashboard in a homelab environment.

## 🚀 Steps

### 1. Configuration
- Verify and fill in necessary secrets in `.env`.
- Ensure `postgres` and `centrifugo` configurations are correct.

### 2. Ignition
// turbo
- Spin up the stack:
  ```powershell
  docker compose up -d --build
  ```

### 3. Health Check
- Verify all services are running:
  ```powershell
  docker compose ps
  ```
- Check logs for any startup errors:
  ```powershell
  docker compose logs -f
  ```

### 4. UI Access
- Navigate to the configured web address (default: `http://localhost:3000`).
- Ensure the dashboard connects to the `sera-core` service.
