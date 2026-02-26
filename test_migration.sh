#!/usr/bin/env bash
# Tests: enveil store creation -> enject migration -> value update
# Requires: enveil, enject, expect, python3

set -euo pipefail

if ! command -v expect &>/dev/null; then
    echo "Error: 'expect' is required. Install with: brew install expect"
    exit 1
fi
if ! command -v enveil &>/dev/null; then
    echo "Error: 'enveil' binary not found in PATH"
    exit 1
fi
if ! command -v enject &>/dev/null; then
    echo "Error: 'enject' binary not found in PATH"
    exit 1
fi

TESTDIR=$(mktemp -d)
cd "$TESTDIR"
echo "Test directory: $TESTDIR"

PASS="testpass"
BEFORE="value_before_migration"
AFTER="value_after_migration_update"

step() { echo; echo "──── $* ────"; }

# ── 1. Create an enveil store ──────────────────────────────────────────────────
step "1. enveil init"
expect <<EOF
set timeout 30
spawn enveil init
expect -re {password: }
send "$PASS\r"
expect -re {password: }
send "$PASS\r"
expect "Initialized"
EOF

# ── 2. Write a secret using the old enveil binary ─────────────────────────────
step "2. enveil set my_secret (value: $BEFORE)"
expect <<EOF
set timeout 30
spawn enveil set my_secret
expect -re {password: }
send "$PASS\r"
expect -re {Value for}
send "$BEFORE\r"
expect "saved"
EOF

step "State — should see .enveil/ only:"
ls -la "$TESTDIR"

# ── 3. First enject command triggers migration prompt ─────────────────────────
step "3. enject list (triggers migration — answering y)"
expect <<EOF
set timeout 30
spawn enject list
expect -re {\[y/N\]}
send "y\r"
expect -re {password: }
send "$PASS\r"
expect "my_secret"
EOF

step "State — should see .enject/ and .enveil.bak/:"
ls -la "$TESTDIR"

# ── 4. Update the secret using enject ─────────────────────────────────────────
step "4. enject set my_secret (value: $AFTER)"
expect <<EOF
set timeout 30
spawn enject set my_secret
expect -re {password: }
send "$PASS\r"
expect -re {Value for}
send "$AFTER\r"
expect "saved"
EOF

# ── 5. Verify the updated value is injected ───────────────────────────────────
step "5. enject run — subprocess should see '$AFTER'"
echo "MY_SECRET=en://my_secret" > .env

cat > check.py <<'PYEOF'
import os
val = os.environ.get("MY_SECRET", "NOT_FOUND")
print(f"MY_SECRET={val}")
PYEOF

expect <<EOF
set timeout 30
spawn enject run -- python3 check.py
expect -re {password: }
send "$PASS\r"
expect "MY_SECRET"
expect eof
EOF

echo ""
echo "Expected:  MY_SECRET=$AFTER"
echo "If you saw the expected value above, migration + update is working."
echo "If you saw '$BEFORE', the update is not persisting after migration."
echo ""
echo "Cleanup: rm -rf $TESTDIR"
