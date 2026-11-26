"""OpenCode WSL snapshot and session-delete operations.

The snapshot is produced by SQLite's online backup API inside the distro, so
the main database and WAL are captured at one point in time. Rust copies the
static snapshot to a local cache and then asks this script to remove the WSL
temporary file. Only `delete_session` writes to the live database.
"""
import glob
import json
import os
import pathlib
import sqlite3
import sys
import tempfile
import time

sys.stdout.reconfigure(encoding="utf-8")

FRAME_CHARS = 64 * 1024
DB_PATH = pathlib.Path(sys.argv[1])
request = json.loads(sys.argv[2])
SNAPSHOT_GLOB = "/tmp/cc-switch-opencode-*.db"


def table_exists(connection, table):
    return bool(
        connection.execute(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
            (table,),
        ).fetchone()[0]
    )


def cleanup_stale_snapshots():
    now = time.time()
    for path in glob.glob(SNAPSHOT_GLOB):
        try:
            if now - os.path.getmtime(path) > 3600:
                os.unlink(path)
        except OSError:
            pass


def open_source():
    """Open the live database without creating it.

    The read-only URI avoids taking a write lock. A read-write connection is
    only a fallback for WAL databases whose `-shm` file cannot be created by a
    read-only process; `mode=rw` still never creates a missing database.
    """
    uri = DB_PATH.as_uri()
    try:
        return sqlite3.connect(uri + "?mode=ro", uri=True, timeout=5.0, isolation_level=None)
    except sqlite3.OperationalError:
        return sqlite3.connect(uri + "?mode=rw", uri=True, timeout=5.0, isolation_level=None)


def make_snapshot():
    cleanup_stale_snapshots()
    fd, temp_path = tempfile.mkstemp(
        prefix="cc-switch-opencode-", suffix=".db", dir="/tmp"
    )
    os.close(fd)
    os.chmod(temp_path, 0o600)
    source = None
    destination = None
    try:
        source = open_source()
        destination = sqlite3.connect(temp_path)
        source.backup(destination)
        destination.close()
        destination = None
        source.close()
        source = None
        return {"path": temp_path}
    except Exception:
        try:
            os.unlink(temp_path)
        except FileNotFoundError:
            pass
        raise
    finally:
        if destination is not None:
            destination.close()
        if source is not None:
            source.close()


def session_schema(connection):
    v2 = table_exists(connection, "session_v2") and table_exists(connection, "session_message")
    return "v2" if v2 else "v1"


def delete_session(connection):
    session_id = request["session_id"]
    schema = session_schema(connection)
    deleted = 0
    connection.execute("BEGIN IMMEDIATE")
    try:
        if schema == "v2":
            deleted += connection.execute(
                "DELETE FROM session_message WHERE session_id = ?", (session_id,)
            ).rowcount
            for child in (
                "session_pending",
                "session_inbox",
                "instruction_entry",
                "instruction_state",
            ):
                if table_exists(connection, child):
                    connection.execute(
                        "DELETE FROM " + child + " WHERE session_id = ?", (session_id,)
                    )
            if table_exists(connection, "event_sequence"):
                connection.execute(
                    "DELETE FROM event_sequence WHERE aggregate_id = ?", (session_id,)
                )
            deleted += connection.execute(
                "DELETE FROM session_v2 WHERE id = ?", (session_id,)
            ).rowcount
            if table_exists(connection, "session"):
                if table_exists(connection, "part"):
                    connection.execute(
                        "DELETE FROM part WHERE session_id = ?", (session_id,)
                    )
                if table_exists(connection, "message"):
                    connection.execute(
                        "DELETE FROM message WHERE session_id = ?", (session_id,)
                    )
                deleted += connection.execute(
                    "DELETE FROM session WHERE id = ?", (session_id,)
                ).rowcount
        else:
            if table_exists(connection, "part"):
                connection.execute("DELETE FROM part WHERE session_id = ?", (session_id,))
            if table_exists(connection, "message"):
                connection.execute("DELETE FROM message WHERE session_id = ?", (session_id,))
            deleted += connection.execute(
                "DELETE FROM session WHERE id = ?", (session_id,)
            ).rowcount
        connection.execute("COMMIT")
    except Exception:
        if connection.in_transaction:
            connection.execute("ROLLBACK")
        raise
    return {"deleted": deleted > 0}


operation = request["operation"]
if operation == "snapshot":
    response = make_snapshot()
elif operation == "delete_session":
    connection = sqlite3.connect(
        DB_PATH.as_uri() + "?mode=rw", uri=True, timeout=2.0, isolation_level=None
    )
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys=ON")
    try:
        response = delete_session(connection)
    finally:
        connection.close()
else:
    raise ValueError("Unsupported OpenCode operation")

for part in json.JSONEncoder(ensure_ascii=False).iterencode(response):
    for start in range(0, len(part), FRAME_CHARS):
        print(json.dumps(part[start:start + FRAME_CHARS], ensure_ascii=False))
