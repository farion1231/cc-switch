"""Fixed read-only Hermes operations; invoked through WSL, never a shell."""
import json
import pathlib
import sqlite3
import sys

sys.stdout.reconfigure(encoding="utf-8")

FRAME_CHARS = 128 * 1024
request = json.loads(sys.argv[2])
connection = sqlite3.connect(
    pathlib.Path(sys.argv[1]).as_uri() + "?mode=ro", uri=True, timeout=2.0,
    isolation_level=None,
)
connection.row_factory = sqlite3.Row
connection.execute("PRAGMA query_only=ON")
connection.execute("BEGIN")


def rows(sql, parameters=()):
    result = []
    for row in connection.execute(sql, parameters):
        item = {key: (None if isinstance(row[key], bytes) else row[key]) for key in row.keys()}
        result.append(item)
    return result


def visible():
    columns = {row["name"] for row in connection.execute("PRAGMA table_info(messages)")}
    if "active" not in columns:
        return ""
    return " AND (active=1 OR compacted=1)" if "compacted" in columns else " AND active=1"


operation = request["operation"]
if operation == "scan":
    sessions = rows("SELECT * FROM sessions ORDER BY rowid DESC LIMIT 500")
    edges = {}
    for name, pick, condition in (
        ("first", "MIN", "role='user'"),
        ("last", "MAX", "role IN ('user','assistant') AND content IS NOT NULL AND content!=''"),
    ):
        edges[name] = rows(
            "SELECT m.session_id,m.content FROM messages m JOIN "
            "(SELECT session_id," + pick + "(id) edge_id FROM messages WHERE "
            + condition + visible()
            + " AND session_id IN (SELECT id FROM sessions ORDER BY rowid DESC LIMIT 500) "
            "GROUP BY session_id) f ON m.id=f.edge_id"
        ) if connection.execute("PRAGMA table_info(messages)").fetchone() else []
    response = {"sessions": sessions, **edges}
elif operation == "messages":
    response = rows(
        "SELECT * FROM messages WHERE session_id=?" + visible() + " ORDER BY id ASC",
        (request["session_id"],),
    )
elif operation == "cell":
    column = request["column"]
    if column not in ("content", "reasoning", "tool_calls"):
        raise ValueError("Unsupported Hermes content column")
    # Fetch the size before the value, preserving the caller's existing limit.
    parameters = (request["id"], request["session_id"])
    size = connection.execute(
        "SELECT length(CAST(" + column + " AS BLOB)) FROM messages WHERE id=? AND session_id=?",
        parameters,
    ).fetchone()
    if size is None:
        raise ValueError("Hermes content is no longer available")
    if (size[0] or 0) > request["max_bytes"]:
        raise ValueError("Hermes content is too large")
    value = connection.execute(
        "SELECT " + column + " FROM messages WHERE id=? AND session_id=?", parameters,
    ).fetchone()[0]
    if isinstance(value, bytes):
        value = value.decode("utf-8", errors="replace")
    if not isinstance(value, str):
        raise ValueError("Hermes content is not text")
    response = value
else:
    raise ValueError("Unsupported Hermes read operation")
connection.rollback()
connection.close()
# Frame the JSON stream, rather than rejecting a whole scan/session before
# Rust can apply its display projection. Each frame is bounded even when JSON
# escaping expands a cell; cell expansion retains its separate max_bytes guard.
# All queries above still use the same fresh, read-only SQLite transaction.
for part in json.JSONEncoder(ensure_ascii=False).iterencode(response):
    for start in range(0, len(part), FRAME_CHARS):
        print(json.dumps(part[start:start + FRAME_CHARS], ensure_ascii=False))
