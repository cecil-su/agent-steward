
CREATE TABLE projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    name_key TEXT NOT NULL UNIQUE CHECK(length(name_key) > 0),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE project_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    revision INTEGER NOT NULL CHECK(revision >= 1),
    change_type TEXT NOT NULL CHECK(change_type IN ('project.created','project.renamed','component.created','source.added','source.removed')),
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(project_id, revision)
);
CREATE TRIGGER projects_id_immutable
BEFORE UPDATE OF id ON projects WHEN NEW.id != OLD.id
BEGIN
    SELECT RAISE(ABORT, 'project id is immutable');
END;

CREATE TABLE components (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(project_id,name_key),
    UNIQUE(id,project_id)
);
CREATE TABLE repositories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    common_dir TEXT NOT NULL UNIQUE,
    common_identity_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE source_roots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    component_id INTEGER NULL,
    repository_id INTEGER NULL REFERENCES repositories(id),
    relative_path TEXT NULL,
    directory_path TEXT NULL,
    directory_identity_json TEXT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(component_id,project_id) REFERENCES components(id,project_id),
    CHECK ((repository_id IS NOT NULL AND relative_path IS NOT NULL AND directory_path IS NULL AND directory_identity_json IS NULL)
        OR (repository_id IS NULL AND relative_path IS NULL AND directory_path IS NOT NULL AND directory_identity_json IS NOT NULL))
);
CREATE UNIQUE INDEX idx_source_git ON source_roots(project_id,coalesce(component_id,0),repository_id,relative_path) WHERE repository_id IS NOT NULL;
CREATE UNIQUE INDEX idx_source_directory ON source_roots(project_id,coalesce(component_id,0),directory_path) WHERE directory_path IS NOT NULL;
CREATE INDEX idx_sources_repository ON source_roots(repository_id);

CREATE TABLE tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NULL REFERENCES projects(id),
    task_key TEXT NULL UNIQUE CHECK(task_key IS NULL OR length(trim(task_key)) > 0),
    title TEXT NULL CHECK(title IS NULL OR length(trim(title)) > 0),
    status TEXT NOT NULL CHECK(status IN ('open','in_progress','blocked','closed')),
    version INTEGER NOT NULL CHECK(version >= 1),
    goal TEXT NULL CHECK(goal IS NULL OR length(trim(goal)) > 0),
    scope TEXT NULL CHECK(scope IS NULL OR length(trim(scope)) > 0),
    acceptance_criteria TEXT NULL CHECK(acceptance_criteria IS NULL OR length(trim(acceptance_criteria)) > 0),
    next_step TEXT NULL CHECK(next_step IS NULL OR length(trim(next_step)) > 0),
    block_reason TEXT NULL,
    block_recovery TEXT NULL,
    current_session_id TEXT NULL,
    repository_path TEXT NULL,
    repository_common_dir TEXT NULL,
    repository_branch TEXT NULL,
    worktree_path TEXT NULL,
    latest_checkpoint_id TEXT NULL,
    closure_outcome TEXT NULL CHECK(closure_outcome IS NULL OR closure_outcome IN ('completed','partial','cancelled','superseded')),
    closure_reason TEXT NULL,
    closed_at TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(id, current_session_id),
    CHECK (
        (status = 'blocked'
            AND block_reason IS NOT NULL AND length(trim(block_reason)) > 0
            AND block_recovery IS NOT NULL AND length(trim(block_recovery)) > 0)
        OR
        (status != 'blocked' AND block_reason IS NULL AND block_recovery IS NULL)
    ),
    CHECK (
        (status = 'closed'
            AND closure_outcome IS NOT NULL
            AND closed_at IS NOT NULL AND length(trim(closed_at)) > 0
            AND (closure_reason IS NULL OR length(trim(closure_reason)) > 0)
            AND (closure_outcome = 'completed' OR closure_reason IS NOT NULL)
            AND (closure_outcome != 'completed' OR (
                title IS NOT NULL AND goal IS NOT NULL AND scope IS NOT NULL
                AND acceptance_criteria IS NOT NULL)))
        OR
        (status != 'closed'
            AND closure_outcome IS NULL AND closure_reason IS NULL AND closed_at IS NULL)
    ),
    CHECK (status != 'closed' OR (current_session_id IS NULL AND next_step IS NULL AND block_reason IS NULL AND block_recovery IS NULL)),
    CHECK ((repository_path IS NULL AND repository_common_dir IS NULL AND repository_branch IS NULL AND worktree_path IS NULL)
        OR (repository_path IS NOT NULL AND repository_common_dir IS NOT NULL AND repository_branch IS NOT NULL AND worktree_path IS NOT NULL)),
    FOREIGN KEY(current_session_id, id) REFERENCES sessions(id, task_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(latest_checkpoint_id, id) REFERENCES checkpoints(id, task_id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    source TEXT NULL,
    external_session_id TEXT NULL,
    continued_from TEXT NULL,
    record_path TEXT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL,
    UNIQUE(id, task_id),
    CHECK(external_session_id IS NULL OR (source IS NOT NULL AND length(trim(source)) > 0)),
    CHECK(continued_from IS NULL OR continued_from != id),
    FOREIGN KEY(continued_from, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE checkpoints (
    id TEXT PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    session_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    completed_json TEXT NOT NULL,
    decisions_json TEXT NOT NULL,
    pending_json TEXT NOT NULL,
    next_step TEXT NOT NULL,
    risks_json TEXT NOT NULL,
    git_head TEXT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(id, task_id),
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE task_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    session_id TEXT NULL,
    note_type TEXT NOT NULL CHECK(note_type IN ('decision','progress','risk')),
    text TEXT NOT NULL CHECK(length(trim(text)) > 0),
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES tasks(id),
    sequence INTEGER NOT NULL,
    change_type TEXT NOT NULL,
    session_id TEXT NULL,
    occurred_at TEXT NOT NULL,
    summary TEXT NOT NULL,
    payload_json TEXT NULL,
    UNIQUE(task_id, sequence),
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id)
);

CREATE TABLE session_imports (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    source_path TEXT NOT NULL,
    media_type TEXT NULL,
    sha256 TEXT NOT NULL,
    content BLOB NOT NULL,
    imported_at TEXT NOT NULL,
    UNIQUE(session_id, sha256)
);

CREATE TABLE session_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    event_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    kind TEXT NULL CHECK(kind IS NULL OR kind IN ('started','resumed','idle','closed','user_message','assistant_message','tool_call','tool_result','error')),
    occurred_at TEXT NULL,
    received_at TEXT NULL,
    UNIQUE(session_id, event_id),
    CHECK ((kind IS NULL AND occurred_at IS NULL AND received_at IS NULL)
        OR (kind IS NOT NULL AND occurred_at IS NOT NULL AND received_at IS NOT NULL))
);
CREATE INDEX idx_session_events_sequence ON session_events(session_id, sequence);

CREATE UNIQUE INDEX idx_tasks_project_identity ON tasks(id,project_id);
CREATE TABLE task_components (
    task_id INTEGER NOT NULL,
    project_id INTEGER NOT NULL,
    component_id INTEGER NOT NULL,
    PRIMARY KEY(task_id,component_id),
    FOREIGN KEY(task_id,project_id) REFERENCES tasks(id,project_id),
    FOREIGN KEY(component_id,project_id) REFERENCES components(id,project_id)
);

CREATE INDEX idx_tasks_project_updated ON tasks(project_id, updated_at, id);
CREATE INDEX idx_tasks_status_updated ON tasks(status, updated_at);
CREATE INDEX idx_tasks_repo_updated ON tasks(repository_common_dir, updated_at);
CREATE INDEX idx_sessions_task_started ON sessions(task_id, started_at);
CREATE UNIQUE INDEX idx_sessions_identity ON sessions(id, task_id);
CREATE UNIQUE INDEX idx_sessions_external ON sessions(source, external_session_id) WHERE external_session_id IS NOT NULL;
CREATE INDEX idx_checkpoints_task_created ON checkpoints(task_id, created_at);
CREATE UNIQUE INDEX idx_checkpoints_identity ON checkpoints(id, task_id);
CREATE INDEX idx_notes_task_created ON task_notes(task_id, created_at);
CREATE INDEX idx_history_task_sequence ON history(task_id, sequence);
CREATE INDEX idx_imports_session_time ON session_imports(session_id, imported_at);

CREATE TRIGGER tasks_id_immutable
BEFORE UPDATE OF id ON tasks
WHEN NEW.id != OLD.id
BEGIN
    SELECT RAISE(ABORT, 'task id is immutable');
END;

CREATE TRIGGER tasks_key_immutable
BEFORE UPDATE OF task_key ON tasks
WHEN OLD.task_key IS NOT NULL AND NEW.task_key IS NOT OLD.task_key
BEGIN
    SELECT RAISE(ABORT, 'task key can only be set once');
END;

CREATE TRIGGER tasks_descriptions_not_cleared
BEFORE UPDATE OF title, goal, scope, acceptance_criteria ON tasks
WHEN (OLD.title IS NOT NULL AND NEW.title IS NULL)
    OR (OLD.goal IS NOT NULL AND NEW.goal IS NULL)
    OR (OLD.scope IS NOT NULL AND NEW.scope IS NULL)
    OR (OLD.acceptance_criteria IS NOT NULL AND NEW.acceptance_criteria IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'task descriptions cannot be cleared once set');
END;

CREATE TRIGGER tasks_current_session_same_task
BEFORE UPDATE OF current_session_id ON tasks
WHEN NEW.current_session_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM sessions s
        WHERE s.id = NEW.current_session_id AND s.task_id = NEW.id AND s.ended_at IS NULL
    ) THEN RAISE(ABORT, 'current session must be active and belong to task') END;
END;

CREATE TRIGGER tasks_latest_checkpoint_same_task
BEFORE UPDATE OF latest_checkpoint_id ON tasks
WHEN NEW.latest_checkpoint_id IS NOT NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM checkpoints c WHERE c.id = NEW.latest_checkpoint_id AND c.task_id = NEW.id
    ) THEN RAISE(ABORT, 'latest checkpoint must belong to task') END;
END;


PRAGMA user_version=4;
