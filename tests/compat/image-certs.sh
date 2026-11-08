#!/usr/bin/env bash
# Certificates for the HTTPS image suite, made fresh for each run (never
# committed): a test CA and a "localhost" certificate it signs (a DNS name
# only, no IP address, so https://127.0.0.1 is a name mismatch), and a
# second CA nobody trusts with its own "localhost" certificate.
#
#   tests/compat/image-certs.sh DIR
set -euo pipefail
dir="$1"
mkdir -p "$dir"
cd "$dir"
days=2
ca() { # name subject
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days "$days" \
    -keyout "$1.key" -out "$1.pem" -subj "/CN=$2" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
}
leaf() { # name ca
  openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout "$1.key" -out "$1.csr" -subj "/CN=localhost" 2>/dev/null
  printf 'subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA "$2.pem" -CAkey "$2.key" -CAcreateserial -days "$days" -extfile "$1.ext" -out "$1.pem" 2>/dev/null
  rm -f "$1.csr" "$1.ext"
}
ca test-ca "NextRS image suite test CA"
leaf localhost test-ca
ca untrusted-ca "Untrusted test CA"
leaf untrusted untrusted-ca
echo "$dir/test-ca.pem"
