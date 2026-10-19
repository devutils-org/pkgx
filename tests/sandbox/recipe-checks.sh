#!/bin/sh
# Runs only inside tests/sandbox/recipe-userns.sh: read-only host, private /tmp,
# loopback-only network. The executed script is the fixture written below.
set -eu
fail() { echo "$1" >&2; exit 1; }
export HOME=/tmp/home
mkdir -p "$HOME" /tmp/www /tmp/tls
cd /tmp/tls
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=localhost \
    -addext subjectAltName=DNS:localhost -keyout key.pem -out cert.pem 2>/dev/null
cat > /tmp/www/install.sh <<'SCRIPT'
set -eu
bin="$HOME/.local/bin"
case "$1" in
install)
    mkdir -p "$bin" "$HOME/.config"
    printf '#!/bin/sh\necho demo\n' > "$bin/demo-tool"
    chmod 0755 "$bin/demo-tool"
    ln -s demo-tool "$bin/dt"
    printf 'greeting = "hi"\n' > "$HOME/.config/demo-tool.toml"
    # Undeclared side effect, visible only to --observe.
    mkdir -p "$HOME/.cache" && printf 'log\n' > "$HOME/.cache/demo-tool.log"
    ;;
uninstall)
    rm -f "$bin/demo-tool" "$bin/dt"
    ;;
esac
SCRIPT
python3 -I -c '
import http.server, ssl, functools
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory="/tmp/www")
server = http.server.HTTPServer(("127.0.0.1", 8443), handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain("/tmp/tls/cert.pem", "/tmp/tls/key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
server.serve_forever()
' 2>/dev/null &
server=$!
trap 'kill $server' EXIT
export CURL_CA_BUNDLE=/tmp/tls/cert.pem
for _ in 1 2 3 4 5 6 7 8 9 10; do
    curl -fsS https://localhost:8443/install.sh >/dev/null 2>&1 && break
    sleep 0.2
done
digest=$(sha256sum /tmp/www/install.sh | cut -d' ' -f1)
recipe() {
    cat <<RECIPE
version = 1
name = "demo-tool"
pinned_version = "1.0.0"
url = "https://localhost:8443/install.sh"
sha256 = "$1"
interpreter = "sh"
install = { args = ["install"] }
uninstall = { args = ["uninstall"] }
resources = [
  { kind = "file", path = "~/.local/bin/demo-tool" },
  { kind = "link", path = "~/.local/bin/dt" },
  { kind = "configuration", path = "~/.config/demo-tool.toml" },
]
RECIPE
}
recipe "$digest" > /tmp/demo.toml
recipe "$(printf '%064d' 1)" > /tmp/tampered.toml
state="--state-dir /tmp/state"
xpac recipe check -f /tmp/demo.toml --json
xpac recipe install -f /tmp/demo.toml --dry-run $state --json
if xpac recipe install -f /tmp/tampered.toml --yes $state; then
    fail 'tampered artifact unexpectedly installed'
fi
[ ! -e "$HOME/.local/bin/demo-tool" ] || fail 'tampered install ran the script'
if printf 'no\n' | xpac recipe install -f /tmp/demo.toml $state; then
    fail 'untrusted install unexpectedly ran'
fi
[ ! -e "$HOME/.local/bin/demo-tool" ] || fail 'unconfirmed install ran the script'
xpac recipe install -f /tmp/demo.toml --yes --observe $state --json > /tmp/install.json
grep -q 'demo-tool.log' /tmp/install.json || fail 'undeclared effect not observed'
[ "$("$HOME/.local/bin/dt")" = demo ] || fail 'installed tool does not run'
xpac recipe list $state --json | grep -q '"classification":"observed"'
if xpac recipe install -f /tmp/demo.toml --yes $state; then
    fail 'duplicate install unexpectedly ran'
fi
cp "$HOME/.local/bin/demo-tool" /tmp/original
printf 'user edit\n' >> "$HOME/.local/bin/demo-tool"
if xpac recipe uninstall demo-tool --yes $state; then
    fail 'modified resource unexpectedly uninstalled'
fi
[ -e "$HOME/.local/bin/demo-tool" ] || fail 'refused uninstall removed a file'
cp /tmp/original "$HOME/.local/bin/demo-tool"
printf 'greeting = "edited"\n' > "$HOME/.config/demo-tool.toml"
xpac recipe uninstall demo-tool --dry-run $state --json
xpac recipe uninstall demo-tool --yes $state --json > /tmp/uninstall.json
grep -q '"success":true' /tmp/uninstall.json
[ ! -e "$HOME/.local/bin/demo-tool" ] && [ ! -L "$HOME/.local/bin/dt" ] || fail 'owned files remain'
grep -q edited "$HOME/.config/demo-tool.toml" || fail 'configuration was not retained'
[ -e "$HOME/.cache/demo-tool.log" ] || fail 'observed path was removed'
[ "$(xpac recipe list $state --json)" = '{"data":[],"schema_version":1}' ] || fail 'receipt remains'
# A pre-existing user file can never be claimed by a recipe.
mkdir -p "$HOME/.local/bin" && printf 'mine\n' > "$HOME/.local/bin/demo-tool"
if xpac recipe install -f /tmp/demo.toml --yes $state; then
    fail 'pre-existing path unexpectedly claimed'
fi
[ "$(cat "$HOME/.local/bin/demo-tool")" = mine ] || fail 'user file changed'
echo 'recipe checks passed'
