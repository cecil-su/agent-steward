"""Synthetic-only Schema 2 -> 5 copy rehearsal, NOT a database migration command.

Usage: python crates/cli/tests/schema2_rehearsal.py OLD_TASKCTL NEW_TASKCTL
Both binaries must be trusted builds. No source/destination DB argument is accepted.
All database, Git and import paths are created inside one disposable sandbox.
Only aggregate checks and executable hashes leave the sandbox; no service is started.
"""

import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from contextlib import closing

TABLES = ("tasks", "sessions", "checkpoints", "task_notes", "history",
          "session_imports", "session_events")
NEW_TABLES = ("projects", "project_history", "components", "repositories",
              "source_roots", "task_components", "project_profiles")
SEQUENCES = {"tasks": "id", "task_notes": "id", "history": "id",
             "session_events": "sequence"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def readonly(path):
    connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
    connection.execute("PRAGMA query_only=ON")
    connection.execute("BEGIN")
    return connection


def schema(connection):
    return connection.execute(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema "
        "WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
    ).fetchall()


def columns(connection, table):
    return [row[1] for row in connection.execute(f'PRAGMA table_info("{table}")')]


def rows(connection, table, fields=None):
    fields = fields or columns(connection, table)
    names = ",".join('"' + name + '"' for name in fields)
    # Each copied table has a unique ID (or session_events.sequence) in column 1.
    return connection.execute(f'SELECT {names} FROM "{table}" ORDER BY 1').fetchall()


def integrity(connection):
    assert connection.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
    assert connection.execute("PRAGMA foreign_key_check").fetchall() == []


def main(old, new, root):
    checks = []
    env = os.environ.copy()
    # Isolate synthetic Git operations from user hooks, signing and configuration.
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    template = root / "empty-git-template"
    template.mkdir()
    env["GIT_TEMPLATE_DIR"] = str(template)

    def owned(path):
        assert path.resolve().is_relative_to(root)
        return path

    def cli(binary, db, *args, body=None, error=None):
        owned(db)
        argv = [str(binary), "--database", str(db), "--json", "--yes"]
        if body is not None:
            argv += ["--input", "-"]
        result = subprocess.run(
            argv + list(map(str, args)), env=env, cwd=root,
            input=None if body is None else json.dumps(body).encode("utf-8"),
            capture_output=True, timeout=30,
        )
        value = json.loads(result.stdout)
        if error:
            assert result.returncode != 0 and value["error"]["code"] == error, value
        else:
            assert result.returncode == 0 and value["ok"], value
        return value["data"]

    def task(binary, db, key):
        return cli(binary, db, "task", "show", key)["task"]

    def version(binary, db, key):
        return task(binary, db, key)["version"]

    def mutate(binary, db, key, *args, body=None):
        return cli(binary, db, *args, "--if-version", version(binary, db, key), body=body)

    source = root / "source.db"
    cli(old, source, "task", "list")
    with closing(readonly(source)) as c:
        assert c.execute("PRAGMA user_version").fetchone() == (2,)
        expected_schema = schema(c)
        assert {r[1] for r in expected_schema if r[0] == "table"} == set(TABLES)

    for key in ("minimal", "active", "blocked", "closed"):
        body = {} if key == "minimal" else {
            "title": "0908｜研究｜Synthetic schema rehearsal " + key,
            "goal": "Synthetic data only", "scope": "Disposable database",
            "acceptanceCriteria": "Preserve every stored field", "nextStep": "Rehearse",
        }
        cli(old, source, "task", "create", key, body=body)
    cli(old, source, "task", "claim", "active", "--session", "session-a", "--if-version", 1)
    mutate(old, source, "active", "task", "checkpoint", "active", "--session", "session-a",
           body={"summary": "Synthetic checkpoint", "completed": ["create"],
                 "decisions": ["no production data"], "pending": ["copy"],
                 "nextStep": "Continue rehearsal", "risks": []})
    mutate(old, source, "active", "task", "resume", "active", "--session", "session-b",
           "--from-session", "session-a", "--take-over")
    mutate(old, source, "active", "session", "bind", "session-b",
           "--source", "generic", "--external-session", "synthetic-external")
    mutate(old, source, "blocked", "task", "claim", "blocked", "--session", "session-blocked")
    mutate(old, source, "blocked", "task", "block", "blocked", "--reason", "Synthetic block",
           "--recovery", "Synthetic recovery")
    mutate(old, source, "closed", "task", "claim", "closed", "--session", "session-closed")
    mutate(old, source, "closed", "task", "close", "closed", "--outcome", "completed")
    payload = root / "synthetic-import.bin"
    payload.write_bytes(b"Synthetic import only\x00\xff\n")
    mutate(old, source, "active", "session", "import", "add", "active", "--session", "session-b",
           "--file", payload, "--confirm-sensitive-content-reviewed")

    event = {"schemaVersion": 1, "sessionId": "session-b", "source": "generic",
             "externalSessionId": "synthetic-external", "eventId": "deleted-event",
             "kind": "idle", "occurredAt": "2026-09-08T00:00:00Z"}
    cli(old, source, "hook", "ingest", body=event)
    mutate(old, source, "active", "hook", "clear", "session-b")
    cli(old, source, "hook", "ingest", body={**event, "eventId": "visible-event"})
    repo = root / "synthetic-repo"
    subprocess.run(["git", "init", "-q", "-b", "rehearsal", str(repo)], env=env, check=True)
    (repo / "README.md").write_text("Synthetic repository only\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(repo), "add", "README.md"], env=env, check=True)
    subprocess.run(["git", "-C", str(repo), "-c", "user.name=Rehearsal", "-c",
                    "user.email=rehearsal@example.invalid", "commit", "-qm", "synthetic"],
                   env=env, check=True)
    mutate(old, source, "active", "worktree", "adopt", "active", "--repo", repo, "--path", repo)
    # Fixture-only metadata: reserve IDs above the present maximum to model historical deletions.
    with closing(sqlite3.connect(source)) as c:
        for table in SEQUENCES:
            c.execute("UPDATE sqlite_sequence SET seq=seq+1000 WHERE name=?", (table,))
        c.commit()

    def copy_rows(snapshot, destination):
        """Exercise the real Rust CLI, including its refusal and publication boundaries."""
        owned(snapshot)
        owned(destination)
        result = subprocess.run(
            [str(new), "--database", str(destination), "--json", "--yes",
             "database", "import-schema2", "--source", str(snapshot)],
            cwd=root, env=env, capture_output=True, timeout=30,
        )
        value = json.loads(result.stdout)
        if not value["ok"]:
            assert result.returncode != 0
            details = value["error"]["details"]
            if details.get("field") == "source" and "Schema 2" in details.get("reason", ""):
                raise ValueError("unsupported source layout")
            if details.get("field") == "database" and "must not exist" in details.get("reason", ""):
                raise ValueError("occupied destination")
            raise AssertionError(value)
        assert result.returncode == 0 and value["data"]["verified"]
        assert value["data"]["externalPathsObserved"] is False
        with closing(readonly(snapshot)) as src, closing(readonly(destination)) as dst:
            assert dst.execute("PRAGMA user_version").fetchone() == (7,)
            assert {r[1] for r in schema(dst) if r[0] == "table"} == set(TABLES + NEW_TABLES)
            for table in TABLES:
                assert rows(src, table) == rows(dst, table, columns(src, table))
            assert rows(src, "sqlite_sequence") == rows(dst, "sqlite_sequence")
            for table in NEW_TABLES:
                assert rows(dst, table) == []
            assert dst.execute("SELECT count(*) FROM tasks WHERE project_id IS NOT NULL").fetchone() == (0,)
            integrity(dst)

    # Keep WAL alive; commit a note through the real old CLI after the last checkpoint.
    snapshot = root / "snapshot.db"
    with closing(sqlite3.connect(source)) as keeper:
        keeper.execute("PRAGMA wal_autocheckpoint=0")
        keeper.execute("SELECT count(*) FROM tasks").fetchone()
        main_hash = digest(source)
        mutate(old, source, "active", "task", "note", "active", "--type", "progress",
               "--text", "Synthetic WAL-only note after checkpoint")
        assert digest(source) == main_hash
        wal = Path(str(source) + "-wal")
        assert wal.stat().st_size > 0
        wal_hash = digest(wal)
        with closing(readonly(source)) as src, closing(sqlite3.connect(snapshot)) as dst:
            src.backup(dst)
            integrity(dst)
            for table in TABLES:
                assert rows(src, table) == rows(dst, table)
        main_only = root / "incorrect-main-only.db"
        shutil.copyfile(source, main_only)
        with closing(readonly(main_only)) as partial, closing(readonly(snapshot)) as complete:
            assert rows(partial, "task_notes") != rows(complete, "task_notes")
        target = root / "migrated-private" / "schema7.db"
        copy_rows(snapshot, target)
        assert digest(source) == main_hash and digest(wal) == wal_hash
        checks += ["WAL-inclusive snapshot; main-only copy demonstrably incomplete",
                   "source main/WAL bytes unchanged during snapshot and copy",
                   "seven tables, BLOB/JSON, IDs, versions, timestamps and four high-water marks preserved",
                   "six new tables empty; project_id NULL; integrity and foreign keys valid"]

    baseline_hash = digest(snapshot)
    for key in ("minimal", "active", "blocked", "closed"):
        before = task(old, snapshot, key)
        after = task(new, target, key)
        assert before == {k: v for k, v in after.items() if k in before}
        assert after["projectId"] is None and after["componentIds"] == []
        assert cli(old, snapshot, "history", key) == cli(new, target, "history", key)
        cli(new, target, "task", "context", key)
        cli(new, target, "task", "notes", key)
    context = cli(new, target, "task", "context", "active")
    assert context["notesSinceCheckpoint"][-1]["text"] == "Synthetic WAL-only note after checkpoint"
    assert context["worktreeStatus"] is not None
    assert cli(old, snapshot, "session", "list") == cli(new, target, "session", "list")
    assert cli(old, snapshot, "session", "import", "list", "session-b") == cli(new, target, "session", "import", "list", "session-b")
    cli(new, target, "doctor")
    checks.append("public Task/History/Session/Import/context readers and live Worktree observation")

    duplicate = cli(new, target, "hook", "ingest", body=event)
    assert duplicate["duplicate"] and duplicate["deleted"]
    cli(new, target, "hook", "ingest", body={**event, "kind": "closed"}, error="HOOK_EVENT_CONFLICT")
    assert len(cli(new, target, "hook", "list", "session-b")["events"]) == 1
    checks.append("Hook tombstone cannot resurrect and conflicting replay is rejected")

    # Writes only touch the disposable migrated DB, never the comparison snapshot.
    active = task(new, target, "active")
    project = cli(new, target, "project", "create", "--name", "Synthetic project")["project"]
    cli(new, target, "task", "project", "active", "--project", "##" + str(project["id"]),
        "--if-version", active["version"])
    changed = task(new, target, "active")
    for field in ("status", "currentSessionId", "worktreePath", "latestCheckpointId"):
        assert active[field] == changed[field]
    cli(new, target, "task", "project", "active", "--clear", "--if-version", active["version"],
        error="VERSION_CONFLICT")
    with closing(readonly(snapshot)) as c:
        highwater = dict(rows(c, "sqlite_sequence"))
    created = cli(new, target, "task", "create", body={})["task"]
    assert created["id"] > highwater["tasks"]
    mutate(new, target, "active", "task", "note", "active", "--type", "decision", "--text", "Synthetic follow-up")
    cli(new, target, "hook", "ingest", body={**event, "eventId": "after-migration"})
    with closing(readonly(target)) as c:
        for table, field in SEQUENCES.items():
            assert c.execute(f'SELECT max({field}) FROM "{table}"').fetchone()[0] > highwater[table]
        integrity(c)
    checks.append("project association preserves execution fields; stale CAS rejected; all four ID sequences advance")
    rejected_hash = digest(target)
    cli(old, target, "task", "create", body={}, error="UNSUPPORTED_SCHEMA_VERSION")
    assert digest(target) == rejected_hash
    cli(new, snapshot, "task", "create", body={}, error="UNSUPPORTED_SCHEMA_VERSION")
    assert digest(snapshot) == baseline_hash
    checks.append("old CLI rejects Schema7 and new CLI rejects Schema2 without implicit upgrade")

    for version_number in (0, 1, 3, 4):
        wrong = root / f"wrong-{version_number}.db"
        with closing(sqlite3.connect(wrong)) as c:
            c.execute(f"PRAGMA user_version={version_number}")
        destination = root / f"wrong-{version_number}-output.db"
        try:
            copy_rows(wrong, destination)
            raise AssertionError("unsupported schema accepted")
        except ValueError as error:
            assert str(error) == "unsupported source layout"
        assert not destination.exists()
    unknown = root / "unknown-layout.db"
    with closing(readonly(snapshot)) as src, closing(sqlite3.connect(unknown)) as dst:
        src.backup(dst)
        dst.execute("CREATE TABLE unexpected(value TEXT)")
    try:
        copy_rows(unknown, root / "unknown-output.db")
        raise AssertionError("unknown layout accepted")
    except ValueError as error:
        assert str(error) == "unsupported source layout"
    assert not (root / "unknown-output.db").exists()
    for index, suffix in enumerate(("", "-wal", "-shm", "-journal")):
        occupied = root / f"occupied-{index}.db"
        marker = Path(str(occupied) + suffix)
        marker.write_bytes(b"synthetic occupied path; do not overwrite")
        original = digest(marker)
        try:
            copy_rows(snapshot, occupied)
            raise AssertionError("occupied destination accepted")
        except ValueError as error:
            assert str(error) == "occupied destination"
        assert digest(marker) == original
        if suffix:
            assert not occupied.exists()
    checks.append("unsupported schema/layout and occupied main/WAL/SHM/journal rejected before destination creation")
    assert digest(snapshot) == baseline_hash
    checks.append("real import-schema2 CLI used; interruption tests live in Rust, not a Python migration kernel")
    with closing(readonly(snapshot)) as c:
        counts = {table: len(rows(c, table)) for table in TABLES}
    return {"checks": checks, "counts": counts, "oldTaskctlSha256": digest(old),
            "newTaskctlSha256": digest(new), "pythonSqliteVersion": sqlite3.sqlite_version,
            "productionDatabaseAccessed": False, "migrationEntryPoint": "database import-schema2"}


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Rehearsal requires assertions; do not use Python -O/PYTHONOPTIMIZE")
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    binaries = [Path(arg).resolve(strict=True) for arg in sys.argv[1:]]
    with tempfile.TemporaryDirectory(prefix="steward-schema2-rehearsal-") as directory:
        result = main(*binaries, Path(directory).resolve())
    result["sandboxRemoved"] = True
    print(json.dumps(result, ensure_ascii=False, indent=2))
