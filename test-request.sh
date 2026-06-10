#!/usr/bin/env bash
set -euo pipefail

HOST=127.0.0.1
PORT=8080

send() {
    # -w1 (idle/read timeout) is supported by GNU netcat, OpenBSD nc, and
    # macOS BSD nc. The server closes the connection after each request, so nc
    # exits on that close; -w1 is just a safety net. (BSD/GNU's -q/-N are not
    # portable, so we avoid them.)
    printf '%b' "$1" | nc -w1 "$HOST" "$PORT"
}

echo "--- Put key=hello value=world ---"
send "Put\r\n5\r\nhello\r\n5\r\nworld\r\n"

echo "--- Put key=foo value=bar ---"
send "Put\r\n3\r\nfoo\r\n3\r\nbar\r\n"

echo "--- Get key=hello ---"
send "Get\r\n5\r\nhello\r\n"

echo "--- Del key=foo ---"
send "Del\r\n3\r\nfoo\r\n"

echo "--- Get deleted key=foo ---"
send "Get\r\n3\r\nfoo\r\n"

echo "--- Bad command ---"
send "Invalid\r\n5\r\nhello\r\n"

echo "--- Malformed: missing CRLF after key ---"
send "Get\r\n5\r\nhello\n"

echo "--- Put: key length mismatch ---"
send "Put\r\n99\r\nhello\r\n5\r\nworld\r\n"

echo "--- Empty request ---"
send ""

echo "Done."
