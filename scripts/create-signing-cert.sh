#!/bin/bash
# Creates the self-signed certificate that release builds are signed with.
#
# macOS remembers Full Disk Access by an app's signature. Signing every
# release with this same certificate keeps Sweep's access across updates.
# The private key lives only in ~/.sweep-signing and in GitHub Secrets.
# Never commit it: anyone with it can sign an app that macOS treats as Sweep.
set -euo pipefail

NAME="Sweep Release Signing"
DIR="${SWEEP_SIGNING_DIR:-$HOME/.sweep-signing}"
# LibreSSL writes .p12 files the macOS keychain can import (OpenSSL 3 doesn't by default).
OPENSSL=/usr/bin/openssl

if [ -e "$DIR/sweep-signing.p12" ]; then
  echo "$DIR already has a certificate. Keep using it: a new one makes macOS forget Full Disk Access once more." >&2
  exit 1
fi

mkdir -p "$DIR"
chmod 700 "$DIR"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
umask 077

cat > "$tmp/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF

"$OPENSSL" req -x509 -newkey rsa:3072 -sha256 -days 7300 -nodes \
  -config "$tmp/cert.cnf" -keyout "$tmp/key.pem" -out "$DIR/certificate.pem" 2>/dev/null
"$OPENSSL" rand -hex 24 | tr -d '\n' > "$DIR/password.txt"
"$OPENSSL" pkcs12 -export -name "$NAME" -inkey "$tmp/key.pem" -in "$DIR/certificate.pem" \
  -passout "file:$DIR/password.txt" -out "$DIR/sweep-signing.p12"

cat <<EOF
Created "$NAME" in $DIR

Add two secrets to the GitHub repository
(Settings → Secrets and variables → Actions → New repository secret):

  MACOS_CERTIFICATE           copy it with:  base64 -i "$DIR/sweep-signing.p12" | pbcopy
  MACOS_CERTIFICATE_PASSWORD  copy it with:  pbcopy < "$DIR/password.txt"

Keep $DIR backed up and private.
EOF
