#!/usr/bin/env bash
set -euo pipefail

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"

send_request() {
    local label="$1"
    local data="$2"
    printf '→ %s\n' "$label"
    if ! printf '%b' "$data" > "/dev/tcp/$HOST/$PORT" 2>/dev/null; then
        printf '  (connection failed — is the server running?)\n'
    fi
    sleep 0.05
}

echo "Sending requests to $HOST:$PORT"
echo "Watch the server logs to see responses."
echo

# --- Valid requests (server should log success) ---

send_request "PUT hello=world (new)" \
    'op\r\nPut\r\n5\r\nhello\r\n5\r\nworld\r\n'

send_request "GET hello (found)" \
    'op\r\nGet\r\n5\r\nhello\r\n'

send_request "PUT hello=newvalue (replace)" \
    'op\r\nPut\r\n5\r\nhello\r\n8\r\nnewvalue\r\n'

send_request "DEL hello (removed)" \
    'op\r\nDel\r\n5\r\nhello\r\n'

send_request "GET hello (not found after del)" \
    'op\r\nGet\r\n5\r\nhello\r\n'

send_request "PUT with embedded CRLF in value (binary-safe)" \
    'op\r\nPut\r\n3\r\nkey\r\n6\r\nv1\r\nv2\r\n'

# --- Invalid requests (server should log parse warnings) ---

send_request "garbage wire prefix" \
    'bogus\r\n'

send_request "unknown operation name" \
    'op\r\nFlip\r\n3\r\nkey\r\n'

send_request "non-numeric key length" \
    'op\r\nPut\r\nNaN\r\nhello\r\n'

send_request "key length declares more bytes than provided" \
    'op\r\nGet\r\n99\r\nhi\r\n'

send_request "missing CRLF after value (truncated)" \
    'op\r\nPut\r\n3\r\nkey\r\n4\r\nvalu'

# --- Non-command (server warns it's not a command) ---

send_request "client sent a SimpleString" \
    'sstr\r\nhi from client\r\n'

echo
echo "Done. Check the server logs."
