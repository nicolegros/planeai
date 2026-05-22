#!/usr/bin/env bash
# migrate-to-sqlite.sh
# One-time local migration: imports projects.json and archived-sessions.json into planeai.db
# then removes the JSON files.

set -euo pipefail

DB_DIR="$HOME/Library/Application Support/planeai"
DB_PATH="$DB_DIR/planeai.db"
PROJECTS_JSON="$HOME/.config/planeai/projects.json"
SESSIONS_JSON="$DB_DIR/archived-sessions.json"

mkdir -p "$DB_DIR"

if [[ -f "$DB_PATH" ]]; then
    echo "⚠️  Database already exists at: $DB_PATH"
    echo "   Delete it first if you want to re-run migration."
    exit 1
fi

echo "Creating database at: $DB_PATH"

sqlite3 "$DB_PATH" <<'SQL'
CREATE TABLE IF NOT EXISTS project (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    repoPath TEXT NOT NULL,
    defaultProvider TEXT NOT NULL,
    defaultAutoApprove INTEGER NOT NULL,
    defaultBranchStrategy TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session (
    id TEXT PRIMARY KEY,
    taskName TEXT NOT NULL,
    branch TEXT NOT NULL,
    provider TEXT NOT NULL,
    state TEXT NOT NULL,
    projectId TEXT REFERENCES project(id) ON DELETE SET NULL,
    projectName TEXT NOT NULL,
    createdAt TEXT NOT NULL,
    completedAt TEXT,
    archivedAt TEXT
);
SQL

# Import projects
if [[ -f "$PROJECTS_JSON" ]]; then
    echo "Importing projects from: $PROJECTS_JSON"
    python3 -c "
import json, sqlite3, sys

db = sqlite3.connect(sys.argv[1])
with open(sys.argv[2]) as f:
    projects = json.load(f)

for p in projects:
    db.execute(
        'INSERT INTO project (id, name, repoPath, defaultProvider, defaultAutoApprove, defaultBranchStrategy) VALUES (?, ?, ?, ?, ?, ?)',
        (p['id'], p['name'], p['repoPath'], p['defaultProvider'], int(p['defaultAutoApprove']), p['defaultBranchStrategy'])
    )

db.commit()
print(f'  Imported {len(projects)} project(s)')
" "$DB_PATH" "$PROJECTS_JSON"
    rm "$PROJECTS_JSON"
    echo "  Deleted: $PROJECTS_JSON"
else
    echo "No projects.json found, skipping."
fi

# Import archived sessions
if [[ -f "$SESSIONS_JSON" ]]; then
    echo "Importing archived sessions from: $SESSIONS_JSON"
    python3 -c "
import json, sqlite3, sys
from datetime import datetime

db = sqlite3.connect(sys.argv[1])
with open(sys.argv[2]) as f:
    sessions = json.load(f)

now = datetime.utcnow().isoformat()
for s in sessions:
    db.execute(
        'INSERT INTO session (id, taskName, branch, provider, state, projectId, projectName, createdAt, completedAt, archivedAt) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
        (s['id'], s['taskName'], s['branch'], s['provider'], s['state'], s.get('projectId'), s['projectName'], now, now, now)
    )

db.commit()
print(f'  Imported {len(sessions)} session(s)')
" "$DB_PATH" "$SESSIONS_JSON"
    rm "$SESSIONS_JSON"
    echo "  Deleted: $SESSIONS_JSON"
else
    echo "No archived-sessions.json found, skipping."
fi

# Clean up empty config dir
if [[ -d "$HOME/.config/planeai" ]] && [[ -z "$(ls -A "$HOME/.config/planeai")" ]]; then
    rmdir "$HOME/.config/planeai"
    echo "  Removed empty: ~/.config/planeai/"
fi

echo ""
echo "✅ Migration complete. Database: $DB_PATH"
echo "   Run: sqlite3 \"$DB_PATH\" '.tables' to verify."
