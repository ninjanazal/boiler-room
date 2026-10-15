CREATE TABLE factories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE TABLE factory_versions (
    id TEXT PRIMARY KEY,
    factory_id TEXT NOT NULL REFERENCES factories(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    definition TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    UNIQUE (factory_id, version)
);
CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    factory_version_id TEXT NOT NULL REFERENCES factory_versions(id),
    state TEXT NOT NULL,
    request TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE TABLE tasks (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    state TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    rework_count INTEGER NOT NULL DEFAULT 0,
    result TEXT,
    updated_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, node_id)
);
CREATE TABLE events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    ts_ms INTEGER NOT NULL,
    node_id TEXT,
    source TEXT NOT NULL,
    kind TEXT NOT NULL
);
CREATE INDEX events_run ON events(run_id, seq);
CREATE TABLE approvals (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    node_id TEXT,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    resolved_ms INTEGER
);
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    node_id TEXT,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
-- Provider session identifiers, so a worker can be resumed without re-running side effects.
CREATE TABLE sessions (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, node_id, session_id)
);
