"""Wire-neutral Python dataclasses mirroring ``rust/proto/plugin/*.proto``.

Each dataclass exposes ``to_wire()`` and ``from_wire()`` helpers so the stdio
JSON-RPC transport can serialise without depending on the generated protobuf
modules at runtime. The gRPC transport converts between these dataclasses and
the generated protobuf messages lazily.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

# --- MemoryBackend ----------------------------------------------------------


@dataclass(slots=True)
class MemoryRecord:
    key: str
    content: str
    metadata: dict[str, str] = field(default_factory=dict)
    created_at: str | None = None  # ISO 8601

    def to_wire(self) -> dict[str, Any]:
        out: dict[str, Any] = {
            "key": self.key,
            "content": self.content,
            "metadata": dict(self.metadata),
        }
        if self.created_at is not None:
            out["created_at"] = self.created_at
        return out

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> MemoryRecord:
        return cls(
            key=str(data["key"]),
            content=str(data["content"]),
            metadata=dict(data.get("metadata") or {}),
            created_at=data.get("created_at"),
        )


@dataclass(slots=True)
class StoreAck:
    key: str

    def to_wire(self) -> dict[str, Any]:
        return {"key": self.key}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> StoreAck:
        return cls(key=str(data["key"]))


@dataclass(slots=True)
class MemoryQuery:
    query: str
    limit: int = 10
    filter: dict[str, str] = field(default_factory=dict)

    def to_wire(self) -> dict[str, Any]:
        return {
            "query": self.query,
            "limit": int(self.limit),
            "filter": dict(self.filter),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> MemoryQuery:
        return cls(
            query=str(data["query"]),
            limit=int(data.get("limit", 10)),
            filter=dict(data.get("filter") or {}),
        )


# --- ContextEngine ----------------------------------------------------------


@dataclass(slots=True)
class IngestMessage:
    session_id: str
    role: str  # "system" | "user" | "assistant" | "tool"
    content: str
    metadata: dict[str, str] = field(default_factory=dict)

    def to_wire(self) -> dict[str, Any]:
        return {
            "session_id": self.session_id,
            "role": self.role,
            "content": self.content,
            "metadata": dict(self.metadata),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> IngestMessage:
        return cls(
            session_id=str(data["session_id"]),
            role=str(data["role"]),
            content=str(data["content"]),
            metadata=dict(data.get("metadata") or {}),
        )


@dataclass(slots=True)
class IngestAck:
    accepted: bool

    def to_wire(self) -> dict[str, Any]:
        return {"accepted": self.accepted}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> IngestAck:
        return cls(accepted=bool(data.get("accepted", False)))


@dataclass(slots=True)
class AssembleBudget:
    session_id: str
    budget_tokens: int
    constraints_json: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {
            "session_id": self.session_id,
            "budget_tokens": int(self.budget_tokens),
            "constraints_json": self.constraints_json,
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AssembleBudget:
        return cls(
            session_id=str(data["session_id"]),
            budget_tokens=int(data["budget_tokens"]),
            constraints_json=str(data.get("constraints_json") or ""),
        )


@dataclass(slots=True)
class ContextSegment:
    kind: str  # "soul" | "working" | "longterm" | "overflow"
    content: str
    tokens: int

    def to_wire(self) -> dict[str, Any]:
        return {"kind": self.kind, "content": self.content, "tokens": int(self.tokens)}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ContextSegment:
        return cls(
            kind=str(data["kind"]),
            content=str(data["content"]),
            tokens=int(data.get("tokens", 0)),
        )


@dataclass(slots=True)
class AssembledContext:
    segments: list[ContextSegment] = field(default_factory=list)
    total_tokens: int = 0

    def to_wire(self) -> dict[str, Any]:
        return {
            "segments": [s.to_wire() for s in self.segments],
            "total_tokens": int(self.total_tokens),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AssembledContext:
        return cls(
            segments=[ContextSegment.from_wire(s) for s in data.get("segments") or []],
            total_tokens=int(data.get("total_tokens", 0)),
        )


@dataclass(slots=True)
class CtxSearchRequest:
    session_id: str
    query: str
    limit: int = 10

    def to_wire(self) -> dict[str, Any]:
        return {
            "session_id": self.session_id,
            "query": self.query,
            "limit": int(self.limit),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> CtxSearchRequest:
        return cls(
            session_id=str(data["session_id"]),
            query=str(data["query"]),
            limit=int(data.get("limit", 10)),
        )


@dataclass(slots=True)
class CtxSearchHit:
    node_id: str
    depth_label: str
    preview: str
    rank: float | None = None

    def to_wire(self) -> dict[str, Any]:
        out: dict[str, Any] = {
            "node_id": self.node_id,
            "depth_label": self.depth_label,
            "preview": self.preview,
        }
        if self.rank is not None:
            out["rank"] = float(self.rank)
        return out

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> CtxSearchHit:
        rank_val = data.get("rank")
        return cls(
            node_id=str(data["node_id"]),
            depth_label=str(data.get("depth_label") or ""),
            preview=str(data.get("preview") or ""),
            rank=float(rank_val) if rank_val is not None else None,
        )


@dataclass(slots=True)
class CtxSearchResponse:
    hits: list[CtxSearchHit] = field(default_factory=list)

    def to_wire(self) -> dict[str, Any]:
        return {"hits": [h.to_wire() for h in self.hits]}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> CtxSearchResponse:
        return cls(hits=[CtxSearchHit.from_wire(h) for h in data.get("hits") or []])


@dataclass(slots=True)
class DescribeRequest:
    session_id: str
    node_id: str

    def to_wire(self) -> dict[str, Any]:
        return {"session_id": self.session_id, "node_id": self.node_id}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> DescribeRequest:
        return cls(session_id=str(data["session_id"]), node_id=str(data["node_id"]))


@dataclass(slots=True)
class DescribeResponse:
    node_id: str
    depth_label: str = ""
    tokens: int = 0
    child_node_ids: list[str] = field(default_factory=list)
    metadata_json: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {
            "node_id": self.node_id,
            "depth_label": self.depth_label,
            "tokens": int(self.tokens),
            "child_node_ids": list(self.child_node_ids),
            "metadata_json": self.metadata_json,
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> DescribeResponse:
        return cls(
            node_id=str(data["node_id"]),
            depth_label=str(data.get("depth_label") or ""),
            tokens=int(data.get("tokens", 0)),
            child_node_ids=[str(x) for x in data.get("child_node_ids") or []],
            metadata_json=str(data.get("metadata_json") or ""),
        )


@dataclass(slots=True)
class ExpandRequest:
    session_id: str
    node_id: str
    max_tokens: int = 0

    def to_wire(self) -> dict[str, Any]:
        return {
            "session_id": self.session_id,
            "node_id": self.node_id,
            "max_tokens": int(self.max_tokens),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ExpandRequest:
        return cls(
            session_id=str(data["session_id"]),
            node_id=str(data["node_id"]),
            max_tokens=int(data.get("max_tokens", 0)),
        )


@dataclass(slots=True)
class ExpandResponse:
    node_id: str
    content: str = ""
    tokens: int = 0
    truncated: bool = False

    def to_wire(self) -> dict[str, Any]:
        return {
            "node_id": self.node_id,
            "content": self.content,
            "tokens": int(self.tokens),
            "truncated": bool(self.truncated),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ExpandResponse:
        return cls(
            node_id=str(data["node_id"]),
            content=str(data.get("content") or ""),
            tokens=int(data.get("tokens", 0)),
            truncated=bool(data.get("truncated", False)),
        )


@dataclass(slots=True)
class StatusResponse:
    fields_json: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {"fields_json": self.fields_json}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> StatusResponse:
        return cls(fields_json=str(data.get("fields_json") or ""))


@dataclass(slots=True)
class DoctorCheck:
    name: str
    severity: str  # "ok" | "warn" | "fail"
    message: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {"name": self.name, "severity": self.severity, "message": self.message}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> DoctorCheck:
        return cls(
            name=str(data["name"]),
            severity=str(data["severity"]),
            message=str(data.get("message") or ""),
        )


@dataclass(slots=True)
class DoctorReport:
    checks: list[DoctorCheck] = field(default_factory=list)

    def to_wire(self) -> dict[str, Any]:
        return {"checks": [c.to_wire() for c in self.checks]}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> DoctorReport:
        return cls(checks=[DoctorCheck.from_wire(c) for c in data.get("checks") or []])


# --- ToolExecutor -----------------------------------------------------------


@dataclass(slots=True)
class ToolDefinition:
    name: str
    description: str = ""
    input_schema_json: str = ""
    risk_level: str = "read"  # "read" | "write" | "exec"

    def to_wire(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "description": self.description,
            "input_schema_json": self.input_schema_json,
            "risk_level": self.risk_level,
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ToolDefinition:
        return cls(
            name=str(data["name"]),
            description=str(data.get("description") or ""),
            input_schema_json=str(data.get("input_schema_json") or ""),
            risk_level=str(data.get("risk_level") or "read"),
        )


@dataclass(slots=True)
class ToolCall:
    tool_name: str
    arguments_json: str
    call_id: str

    def to_wire(self) -> dict[str, Any]:
        return {
            "tool_name": self.tool_name,
            "arguments_json": self.arguments_json,
            "call_id": self.call_id,
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ToolCall:
        return cls(
            tool_name=str(data["tool_name"]),
            arguments_json=str(data.get("arguments_json") or ""),
            call_id=str(data["call_id"]),
        )


@dataclass(slots=True)
class ToolResult:
    call_id: str
    content_json: str = ""
    is_error: bool = False

    def to_wire(self) -> dict[str, Any]:
        return {
            "call_id": self.call_id,
            "content_json": self.content_json,
            "is_error": bool(self.is_error),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> ToolResult:
        return cls(
            call_id=str(data["call_id"]),
            content_json=str(data.get("content_json") or ""),
            is_error=bool(data.get("is_error", False)),
        )


# --- SandboxProvider --------------------------------------------------------


@dataclass(slots=True)
class SandboxExecRequest:
    command: list[str]
    sandbox_id: str = ""
    env: dict[str, str] = field(default_factory=dict)
    workdir: str = ""
    time_limit_seconds: int = 0
    memory_limit_bytes: int = 0

    def to_wire(self) -> dict[str, Any]:
        return {
            "sandbox_id": self.sandbox_id,
            "command": list(self.command),
            "env": dict(self.env),
            "workdir": self.workdir,
            "time_limit_seconds": int(self.time_limit_seconds),
            "memory_limit_bytes": int(self.memory_limit_bytes),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> SandboxExecRequest:
        return cls(
            command=[str(x) for x in data.get("command") or []],
            sandbox_id=str(data.get("sandbox_id") or ""),
            env=dict(data.get("env") or {}),
            workdir=str(data.get("workdir") or ""),
            time_limit_seconds=int(data.get("time_limit_seconds", 0)),
            memory_limit_bytes=int(data.get("memory_limit_bytes", 0)),
        )


@dataclass(slots=True)
class SandboxExecResult:
    sandbox_id: str
    stdout: str = ""
    stderr: str = ""
    exit_code: int = 0
    duration_seconds: float = 0.0

    def to_wire(self) -> dict[str, Any]:
        return {
            "sandbox_id": self.sandbox_id,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "exit_code": int(self.exit_code),
            "duration_seconds": float(self.duration_seconds),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> SandboxExecResult:
        return cls(
            sandbox_id=str(data["sandbox_id"]),
            stdout=str(data.get("stdout") or ""),
            stderr=str(data.get("stderr") or ""),
            exit_code=int(data.get("exit_code", 0)),
            duration_seconds=float(data.get("duration_seconds", 0.0)),
        )


# --- AuthProvider -----------------------------------------------------------


@dataclass(slots=True)
class AuthRequest:
    subject: str
    credential: str

    def to_wire(self) -> dict[str, Any]:
        return {"subject": self.subject, "credential": self.credential}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AuthRequest:
        return cls(
            subject=str(data["subject"]),
            credential=str(data.get("credential") or ""),
        )


@dataclass(slots=True)
class AuthResponse:
    authenticated: bool
    subject_id: str = ""
    claims: dict[str, str] = field(default_factory=dict)
    reason: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {
            "authenticated": bool(self.authenticated),
            "subject_id": self.subject_id,
            "claims": dict(self.claims),
            "reason": self.reason,
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AuthResponse:
        return cls(
            authenticated=bool(data.get("authenticated", False)),
            subject_id=str(data.get("subject_id") or ""),
            claims=dict(data.get("claims") or {}),
            reason=str(data.get("reason") or ""),
        )


@dataclass(slots=True)
class AuthzRequest:
    subject_id: str
    action: str
    resource: str
    context: dict[str, str] = field(default_factory=dict)

    def to_wire(self) -> dict[str, Any]:
        return {
            "subject_id": self.subject_id,
            "action": self.action,
            "resource": self.resource,
            "context": dict(self.context),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AuthzRequest:
        return cls(
            subject_id=str(data["subject_id"]),
            action=str(data["action"]),
            resource=str(data["resource"]),
            context=dict(data.get("context") or {}),
        )


@dataclass(slots=True)
class AuthzResponse:
    allow: bool
    reason: str = ""

    def to_wire(self) -> dict[str, Any]:
        return {"allow": bool(self.allow), "reason": self.reason}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> AuthzResponse:
        return cls(
            allow=bool(data.get("allow", False)),
            reason=str(data.get("reason") or ""),
        )


# --- SecretProvider ---------------------------------------------------------


@dataclass(slots=True)
class SecretRef:
    name: str

    def to_wire(self) -> dict[str, Any]:
        return {"name": self.name}

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> SecretRef:
        return cls(name=str(data["name"]))


@dataclass(slots=True)
class SecretValue:
    name: str
    value: bytes = b""
    metadata: dict[str, str] = field(default_factory=dict)

    def to_wire(self) -> dict[str, Any]:
        import base64

        return {
            "name": self.name,
            "value_b64": base64.b64encode(self.value).decode("ascii"),
            "metadata": dict(self.metadata),
        }

    @classmethod
    def from_wire(cls, data: dict[str, Any]) -> SecretValue:
        import base64

        b64 = data.get("value_b64")
        raw = base64.b64decode(b64) if b64 else b""
        return cls(
            name=str(data["name"]),
            value=raw,
            metadata=dict(data.get("metadata") or {}),
        )


__all__ = [
    "AssembleBudget",
    "AssembledContext",
    "AuthRequest",
    "AuthResponse",
    "AuthzRequest",
    "AuthzResponse",
    "ContextSegment",
    "CtxSearchHit",
    "CtxSearchRequest",
    "CtxSearchResponse",
    "DescribeRequest",
    "DescribeResponse",
    "DoctorCheck",
    "DoctorReport",
    "ExpandRequest",
    "ExpandResponse",
    "IngestAck",
    "IngestMessage",
    "MemoryQuery",
    "MemoryRecord",
    "SandboxExecRequest",
    "SandboxExecResult",
    "SecretRef",
    "SecretValue",
    "StatusResponse",
    "StoreAck",
    "ToolCall",
    "ToolDefinition",
    "ToolResult",
]
