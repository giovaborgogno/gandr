#!/usr/bin/env bash
# Build the fixture repo the README demo records against, then you can run:
#   cargo build --release && ./docs/demo-setup.sh && vhs docs/demo.tape
# It creates a tiny "todo-service" repo in /tmp/gandr-demo with a committed
# baseline plus uncommitted "agent-style" edits (a new file, word-level tweaks,
# added methods) so the diff and the repo browser both have something to show.
set -euo pipefail

D=/tmp/gandr-demo
rm -rf "$D"
mkdir -p "$D/app" "$D/tests"
cd "$D"
git init -q
git config user.name "Ada"
git config user.email "ada@example.com"

# ---------- baseline (the committed "before") ----------
cat > app/server.py <<'PY'
"""A tiny HTTP API for the todo service."""
from http.server import BaseHTTPRequestHandler
import json

from app.store import TodoStore

store = TodoStore()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/todos":
            todos = store.all()
            self.send(200, todos)
        else:
            self.send(404, {"error": "not found"})

    def send(self, code, body):
        payload = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(payload)
PY

cat > app/store.py <<'PY'
"""In-memory todo storage."""


class TodoStore:
    def __init__(self):
        self._items = []

    def all(self):
        return self._items

    def add(self, title):
        item = {"id": len(self._items) + 1, "title": title, "done": False}
        self._items.append(item)
        return item
PY

cat > app/models.py <<'PY'
from dataclasses import dataclass


@dataclass
class Todo:
    id: int
    title: str
    done: bool = False
PY

cat > tests/test_store.py <<'PY'
from app.store import TodoStore


def test_add_assigns_id():
    store = TodoStore()
    first = store.add("write tests")
    assert first["id"] == 1
    assert first["done"] is False
PY

cat > README.md <<'MD'
# todo-service

A minimal todo API used as a teaching example.

## Run

    python -m app

## Endpoints

- `GET /todos` — list todos
MD

git add -A
git commit -qm "initial todo service"

# ---------- uncommitted edits (the "after" the agent made) ----------
cat > app/server.py <<'PY'
"""A tiny HTTP API for the todo service."""
from http.server import BaseHTTPRequestHandler
import json

from app.store import TodoStore
from app.cache import cache

store = TodoStore()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/todos":
            todos = cache.get_or_set("todos", store.all)
            self.send(200, {"todos": todos, "count": len(todos)})
        else:
            self.send(404, {"error": "unknown route"})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or "{}")
        item = store.add(body["title"])
        cache.clear()
        self.send(201, item)

    def send(self, code, body):
        payload = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(payload)
PY

cat > app/cache.py <<'PY'
"""A dead-simple process-local cache with manual invalidation."""


class Cache:
    def __init__(self):
        self._data = {}

    def get_or_set(self, key, producer):
        if key not in self._data:
            self._data[key] = producer()
        return self._data[key]

    def clear(self):
        self._data.clear()


cache = Cache()
PY

cat > app/store.py <<'PY'
"""In-memory todo storage."""


class TodoStore:
    def __init__(self):
        self._items = []

    def all(self):
        return list(self._items)

    def get(self, todo_id):
        return next((i for i in self._items if i["id"] == todo_id), None)

    def add(self, title):
        item = {"id": len(self._items) + 1, "title": title, "done": False}
        self._items.append(item)
        return item
PY

cat > README.md <<'MD'
# todo-service

A minimal, in-memory todo API used as a teaching example.

## Run

    python -m app

## Endpoints

- `GET /todos` — list todos (cached)
- `POST /todos` — create a todo
MD

echo "demo repo ready at $D"
