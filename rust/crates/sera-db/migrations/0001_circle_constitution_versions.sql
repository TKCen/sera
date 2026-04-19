-- Migration 0001: circle_constitution_versions
-- Audit trail for per-circle constitution document updates.
-- Bead: sera-8d1.4 (GH#147)

-- UP

CREATE TABLE IF NOT EXISTS circle_constitution_versions (
    circle_id  TEXT        NOT NULL,
    version    INTEGER     NOT NULL,
    text_hash  TEXT        NOT NULL,
    changed_by TEXT        NOT NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (circle_id, version)
);

CREATE INDEX IF NOT EXISTS idx_ccv_circle_id
    ON circle_constitution_versions (circle_id);

-- DOWN

-- DROP TABLE IF EXISTS circle_constitution_versions;
