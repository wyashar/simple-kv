#!/usr/bin/env bash
set -euo pipefail

HOST=127.0.0.1
PORT=8080

# Wire format (per request):
#   [2-byte big-endian length][payload]
# where the payload is the length-prefixed command:
#   <oplen>\r\n<op>\r\n<keylen>\r\n<key>\r\n[<vallen>\r\n<value>\r\n]
#
# The 2-byte envelope lets the server consume a whole request even when the
# payload inside is malformed, so a recoverable error (bad op, missing CRLF,
# truncated field) gets an error response and the connection stays in sync.
# Only a broken *envelope* (lying/short length) is unrecoverable and closes it.
#
# frame() computes the envelope length from the real byte count, so it stays
# binary-safe and CRLF-safe. Everything is staged as hex and emitted once via
# `xxd -r -p` to avoid the shell mangling NUL/newlines.

# frame <payload-with-printf-escapes>  ->  hex string of [len][payload]
frame() {
    local hex nbytes
    hex=$(printf '%b' "$1" | xxd -p | tr -d '\n')
    nbytes=$(( ${#hex} / 2 ))
    printf '%04x%s' "$nbytes" "$hex"
}

# pipeline <payload>...  -> frame each request and send them ALL down ONE
# connection. The server keeps its KvStore per-connection, so state set by an
# earlier request is visible to a later one in the same call. -w1 (idle timeout)
# is portable across GNU/OpenBSD/macOS netcat.
pipeline() {
    local allhex="" p
    for p in "$@"; do
        allhex+=$(frame "$p")
    done
    printf '%s' "$allhex" | xxd -r -p | nc -w1 "$HOST" "$PORT"
}

echo "=== All recoverable requests share ONE connection ==="
echo "Sent, in order (responses stream back below in the same order):"
echo "   1. Put hello=world                  -> +"
echo "   2. Get hello                        -> \$5 world"
echo "   3. Put foo=bar                      -> +"
echo "   4. Get foo                          -> \$3 bar"
echo "   5. Del foo                          -> +"
echo "   6. Get foo                          -> !"
echo "   7. Put k=v1                         -> +"
echo "   8. Put k=v2                         -> +"
echo "   9. Get k                            -> \$2 v2"
echo "  10. Put bin=a<CRLF>b                 -> +"
echo "  11. Get bin                          -> \$4 a<CRLF>b"
echo "  12. Get missing                      -> !"
echo "  13. Invalid (bad command)            -> -BadOperation"
echo "  14. Put, key len 99 > payload        -> -PayloadTruncated"
echo "  15. Empty payload (zero-len env)     -> -MissingCrlf"
echo "  16. Get hello (AFTER 3 errors)       -> \$5 world   <- proves connection survived"
echo "--- responses ---"
pipeline \
    "3\r\nPut\r\n5\r\nhello\r\n5\r\nworld\r\n" \
    "3\r\nGet\r\n5\r\nhello\r\n" \
    "3\r\nPut\r\n3\r\nfoo\r\n3\r\nbar\r\n" \
    "3\r\nGet\r\n3\r\nfoo\r\n" \
    "3\r\nDel\r\n3\r\nfoo\r\n" \
    "3\r\nGet\r\n3\r\nfoo\r\n" \
    "3\r\nPut\r\n1\r\nk\r\n2\r\nv1\r\n" \
    "3\r\nPut\r\n1\r\nk\r\n2\r\nv2\r\n" \
    "3\r\nGet\r\n1\r\nk\r\n" \
    "3\r\nPut\r\n3\r\nbin\r\n4\r\na\r\nb\r\n" \
    "3\r\nGet\r\n3\r\nbin\r\n" \
    "3\r\nGet\r\n7\r\nmissing\r\n" \
    "7\r\nInvalid\r\n5\r\nhello\r\n" \
    "3\r\nPut\r\n99\r\nhello\r\n5\r\nworld\r\n" \
    "" \
    "3\r\nGet\r\n5\r\nhello\r\n"
echo

echo "=== Unrecoverable (lying envelope) gets its OWN connection ==="
echo "Envelope claims 100 bytes but only 4 are sent, then EOF."
echo "Server's read_exact on the payload fails -> IoError -> connection closed (no response)."
# 0x0064 = 100-byte envelope, followed by only 4 payload bytes.
printf '%s' "0064deadbeef" | xxd -r -p | nc -w1 "$HOST" "$PORT"
echo

echo "Done."
