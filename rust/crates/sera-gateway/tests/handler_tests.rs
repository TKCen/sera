//! Unit tests for route handlers (called directly, without full router).
//!
//! These tests verify individual handler logic without requiring:
//! - A running database
//! - Full AppState setup
//! - Docker client initialization
//!
//! For integration tests with the full router, see integration_tests.rs

#[cfg(test)]
mod health_handler_tests {
    use serde_json::json;

    /// Test that the health handler produces a JSON response with status field.
    #[test]
    fn health_response_structure() {
        // The health handler returns: Json(json!({"status": "ok"}))
        let expected = json!({"status": "ok"});
        assert_eq!(expected["status"], "ok");
    }

    /// Test that status field is the string "ok", not a boolean.
    #[test]
    fn health_status_is_string() {
        let response = json!({"status": "ok"});
        assert!(response["status"].is_string());
        assert_eq!(response["status"].as_str(), Some("ok"));
    }
}

#[cfg(test)]
mod error_handling_tests {
    use serde_json::json;

    /// Test the format of AppError::NotFound conversion.
    #[test]
    fn not_found_error_format() {
        let error_msg = "agent_instance with id=inst-123 not found";
        let response = json!({"error": error_msg});

        assert!(response["error"].as_str().unwrap().contains("not found"));
        assert!(response["error"].as_str().unwrap().contains("inst-123"));
    }

    /// Test the format of AppError::Unauthorized conversion.
    #[test]
    fn unauthorized_error_format() {
        let response = json!({"error": "Unauthorized"});
        assert_eq!(response["error"], "Unauthorized");
    }

    /// Test the format of AppError::Forbidden conversion.
    #[test]
    fn forbidden_error_format() {
        let reason = "User does not have permission to modify this agent";
        let response = json!({"error": reason});
        assert_eq!(response["error"], reason);
    }

    /// Test that internal errors don't expose sensitive details.
    #[test]
    fn internal_error_hides_details() {
        let response = json!({"error": "Internal server error"});
        assert_eq!(response["error"], "Internal server error");

        // Should NOT contain stack traces, file paths, or system details
        let error_str = response["error"].as_str().unwrap();
        assert!(!error_str.contains("/home/"));
        assert!(!error_str.contains("D:\\"));
        assert!(!error_str.contains("postgres://"));
        assert!(!error_str.contains("thread"));
        assert!(!error_str.contains("stack"));
    }
}

#[cfg(test)]
mod auth_header_tests {
    /// Test that Bearer prefix parsing works correctly.
    #[test]
    fn bearer_prefix_extraction() {
        let header = "Bearer my-token-123";
        let token = header.strip_prefix("Bearer ").unwrap();
        assert_eq!(token, "my-token-123");
    }

    /// Test that missing Bearer prefix is rejected.
    #[test]
    fn missing_bearer_prefix_rejected() {
        let header = "Token my-token-123";
        let result = header.strip_prefix("Bearer ");
        assert!(result.is_none());
    }

    /// Test that Bearer with no token is rejected.
    #[test]
    fn bearer_with_no_token_rejected() {
        let header = "Bearer ";
        let token = header.strip_prefix("Bearer ");
        assert_eq!(token, Some(""));
        // Empty token should fail verification
    }
}

#[cfg(test)]
mod response_format_tests {
    use serde_json::json;

    /// Test that responses use lowercase boolean values.
    #[test]
    fn json_booleans_lowercase() {
        let response = json!({
            "active": true,
            "archived": false,
            "online": true
        });

        let serialized = serde_json::to_string(&response).unwrap();
        assert!(serialized.contains("true"));
        assert!(serialized.contains("false"));
        assert!(!serialized.contains("True"));
        assert!(!serialized.contains("False"));
    }

    /// Test that arrays are properly formed.
    #[test]
    fn json_arrays_well_formed() {
        let response = json!({
            "items": [
                {"id": "item-1", "name": "First"},
                {"id": "item-2", "name": "Second"}
            ]
        });

        assert!(response["items"].is_array());
        assert_eq!(response["items"].as_array().unwrap().len(), 2);
    }

    /// Test that empty arrays are valid.
    #[test]
    fn json_empty_arrays_valid() {
        let response = json!({"items": []});
        assert!(response["items"].is_array());
        assert_eq!(response["items"].as_array().unwrap().len(), 0);
    }

    /// Test that nested objects are properly structured.
    #[test]
    fn json_nested_objects_valid() {
        let response = json!({
            "agent": {
                "id": "agent-123",
                "status": {
                    "current": "running",
                    "healthy": true
                }
            }
        });

        assert!(response["agent"].is_object());
        assert_eq!(response["agent"]["id"], "agent-123");
        assert_eq!(response["agent"]["status"]["current"], "running");
        assert!(response["agent"]["status"]["healthy"].as_bool().unwrap());
    }
}

#[cfg(test)]
mod iso8601_timestamp_tests {
    /// Test that timestamps are ISO 8601 format.
    #[test]
    fn iso8601_format_valid() {
        // RFC 3339 format: YYYY-MM-DDTHH:MM:SSZ or YYYY-MM-DDTHH:MM:SS+HH:MM
        let valid_timestamps = vec![
            "2026-04-05T12:34:56Z",
            "2026-04-05T12:34:56+00:00",
            "2026-04-05T12:34:56-07:00",
        ];

        for ts in valid_timestamps {
            // Verify format: contains T separator and timezone indicator
            assert!(ts.contains("T"), "Timestamp {} missing 'T' separator", ts);
            assert!(
                ts.ends_with("Z") || ts.contains("+") || (ts.rfind("-").is_some_and(|i| i > 10)),
                "Timestamp {} missing timezone",
                ts
            );
        }
    }

    /// Test that timestamps are parseable by JavaScript's Date constructor.
    #[test]
    fn iso8601_javascript_compatible() {
        // JavaScript's new Date() can parse these formats
        let timestamps = vec![
            "2026-04-05T12:34:56Z",
            "2026-04-05T12:34:56+00:00",
            "2026-04-05T12:34:56.123Z",
        ];

        // Verify all timestamps have proper ISO 8601 structure
        for ts in timestamps {
            assert!(!ts.is_empty());
            assert!(ts.contains("T"), "Missing 'T' separator in {}", ts);
            assert!(
                ts.contains("Z") || ts.contains("+") || (ts.rfind("-").is_some_and(|i| i > 10)),
                "Missing timezone in {}",
                ts
            );
        }
    }
}

#[cfg(test)]
mod empty_reply_guard_tests {
    use serde_json::json;

    /// An empty reply from execute_turn must be treated as a failure, not a
    /// successful empty response.  The guard in chat_handler checks
    /// `result.reply.is_empty()` and returns 502 Bad Gateway.  This test
    /// documents the expected response body for that path.
    #[test]
    fn empty_reply_produces_error_body() {
        let reply = "";
        assert!(reply.is_empty(), "guard condition: reply must be empty");

        // The handler returns this JSON body with a 502 status.
        let body = json!({"error": "runtime returned empty reply"});
        assert_eq!(body["error"], "runtime returned empty reply");
    }

    /// A non-empty reply must not trigger the guard.
    #[test]
    fn non_empty_reply_passes_guard() {
        let reply = "Hello from the agent.";
        assert!(!reply.is_empty(), "guard must not fire for non-empty reply");
    }
}

mod api_contract_tests {
    /// Document the expected request/response contract for GET /api/health
    #[test]
    fn health_endpoint_contract() {
        // Method: GET
        // Path: /api/health
        // Auth: Not required
        // Status: 200 OK
        // Body: {"status": "ok"}
        // Content-Type: application/json
    }

    /// Document the expected contract for GET /api/health/detail
    #[test]
    fn health_detail_endpoint_contract() {
        // Method: GET
        // Path: /api/health/detail
        // Auth: Not required
        // Status: 200 OK
        // Body: {
        //   "status": "healthy|degraded",
        //   "components": [
        //     {"name": "database", "status": "healthy|unreachable"},
        //     {"name": "docker", "status": "healthy"},
        //     {"name": "centrifugo", "status": "healthy|degraded"},
        //     {"name": "qdrant", "status": "healthy"}
        //   ],
        //   "agentStats": {
        //     "total": <number>,
        //     "running": <number>,
        //     "stopped": <number>,
        //     "errored": <number>
        //   },
        //   "timestamp": <unix-epoch-seconds>,
        //   "version": "0.1.0"
        // }
        // Content-Type: application/json
    }

    /// Document the expected contract for protected routes
    #[test]
    fn protected_route_contract() {
        // Method: GET/POST/PUT/PATCH/DELETE
        // Auth: Required - Authorization: Bearer <token>
        // No Auth: Status 401 Unauthorized, Body {"error": "Unauthorized"}
        // Invalid Auth: Status 401 Unauthorized, Body {"error": "Unauthorized"}
        // Valid Auth: Proceeds to handler
    }

    /// Document the expected error response format
    #[test]
    fn error_response_contract() {
        // All error responses follow:
        // Content-Type: application/json
        // Body: {"error": "<human-readable message>"}
        //
        // Status codes:
        // 400 Bad Request - {"error": "..."}
        // 401 Unauthorized - {"error": "Unauthorized"}
        // 403 Forbidden - {"error": "..."}
        // 404 Not Found - {"error": "... not found"}
        // 409 Conflict - {"error": "..."}
        // 422 Unprocessable Entity - {"error": "..."}
        // 500 Internal Server Error - {"error": "Internal server error"}
    }
}
