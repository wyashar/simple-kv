#!/usr/bin/env bash
set -euo pipefail

HOST=127.0.0.1
PORT=8080

# Each send() opens ONE connection. The server keeps its KvStore *per connection*,
# so commands that depend on each other (Put then Get) MUST be pipelined into the
# same send() — across separate connections the store starts empty.
#
# -w1 (idle timeout) is portable across GNU/OpenBSD/macOS netcat. The server
# closes the connection once the client hangs up, so nc exits on that.
send() {
    printf '%b' "$1" | nc -w1 "$HOST" "$PORT"
}

echo "=== Put hello=world, then Get hello  (expect +  then  \$5 world) ==="
send "Put\r\n5\r\nhello\r\n5\r\nworld\r\nGet\r\n5\r\nhello\r\n"
echo

echo "=== Put foo=bar, Get foo, Del foo, Get foo  (expect +  \$3 bar  +  !) ==="
send "Put\r\n3\r\nfoo\r\n3\r\nbar\r\nGet\r\n3\r\nfoo\r\nDel\r\n3\r\nfoo\r\nGet\r\n3\r\nfoo\r\n"
echo

echo "=== Overwrite: Put k=v1, Put k=v2, Get k  (expect +  +  \$2 v2) ==="
send "Put\r\n1\r\nk\r\n2\r\nv1\r\nPut\r\n1\r\nk\r\n2\r\nv2\r\nGet\r\n1\r\nk\r\n"
echo

echo "=== Binary-safe value (value itself contains CRLF): Put bin, Get bin  (expect +  \$4 a<CRLF>b) ==="
send "Put\r\n3\r\nbin\r\n4\r\na\r\nb\r\nGet\r\n3\r\nbin\r\n"
echo

echo "=== Get a key that was never set  (expect !) ==="
send "Get\r\n7\r\nmissing\r\n"
echo

echo "=== Bad command  (expect Error) ==="
send "Invalid\r\n5\r\nhello\r\n"
echo

echo "=== Malformed: key length larger than payload  (expect Error: truncated) ==="
send "Put\r\n99\r\nhello\r\n5\r\nworld\r\n"
echo

echo "=== Empty request  (expect Error / nothing) ==="
send ""
echo

echo "Done."
