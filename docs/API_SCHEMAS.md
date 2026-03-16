# SERA API Schemas

This document outlines the API endpoints for the SERA system, focusing on the Core Orchestrator.

## Core Orchestrator API (`sera-core`)

The Core Orchestrator serves as the central hub for managing the system's state and communicating with other services.

### Health Check

Returns the current status of the service.

- **URL:** `/api/health`
- **Method:** `GET`
- **Response Format:** `JSON`
- **Success Response:**
  - **Code:** 200
  - **Content:**
    ```json
    {
      "status": "ok",
      "service": "sera-core",
      "timestamp": "2023-10-27T10:00:00.000Z"
    }
    ```

### Planned Endpoints (v1.1)

#### Workspace Scanning

Trigger a scan of the current workspace file system.

- **URL:** `/api/workspace/scan`
- **Method:** `POST`
- **Response Format:** `JSON`

#### Message Dispatch

Send a message to the SERA neural link.

- **URL:** `/api/messages/send`
- **Method:** `POST`
- **Request Body:**
  ```json
  {
    "message": "string"
  }
  ```

---

## Web UI API (`sera-web`)

The Web UI provides its own internal API routes for client-side operations.

### Health Check

- **URL:** `/api/health`
- **Method:** `GET`
- **Response Format:** `JSON`
- **Success Response:**
  - **Code:** 200
  - **Content:**
    ```json
    {
      "status": "ok",
      "service": "sera-web",
      "timestamp": "2023-10-27T10:00:00.000Z"
    }
    ```
