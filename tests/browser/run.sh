#!/bin/sh
# Runs the page in a real browser, on a Docker network with no route to the
# internet (--internal), against the real server image and a throwaway
# Postgres loaded with the invented example data and with hostile text.
#
#   tests/browser/run.sh smoke               the smoke test, on the example board and
#                                            on the hostile one (exit code 1 on failure)
#   tests/browser/run.sh capture <out-dir>   the screenshots and the transition
#                                            frames of docs/visuals
#
# Everything it creates is named enlightenviz-closed-* or enlightenviz-*:test,
# and the containers (with the anonymous volume Postgres makes), the network
# and the scratch directory are removed on exit. The two images stay, to be
# reused; remove them with
#   docker rmi enlightenviz-image:browser-test enlightenviz-browser:test
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MODE="${1:?usage: run.sh smoke | capture <out-dir>}"
OUT="${2:-}"
NET=enlightenviz-closed
PG=enlightenviz-closed-pg
SERVER=enlightenviz-closed-server
POSTGRES=postgres:18.7-alpine3.24@sha256:75dcac3dfb1cd01ca3536910bbc84128759362f8b118bf8c55b38a94f68f3b73
SCRATCH="$(mktemp -d)"

cleanup() {
  docker rm -fv "$SERVER" "$PG" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

docker build -q -t enlightenviz-image:browser-test "$ROOT" >/dev/null
docker build -q -t enlightenviz-browser:test "$ROOT/tests/browser" >/dev/null

# --internal: the containers can reach each other and nothing else.
docker network create --internal "$NET" >/dev/null
docker run -d --name "$PG" --network "$NET" -e POSTGRES_PASSWORD=throwaway-test-only "$POSTGRES" >/dev/null
until docker exec "$PG" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
# The image's entrypoint restarts Postgres once after initialising; wait for the second start.
sleep 2
until docker exec "$PG" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
docker exec -i "$PG" psql -U postgres -v ON_ERROR_STOP=1 -q <"$ROOT/examples/earthquakes/schema.sql" >/dev/null
docker exec -i "$PG" psql -U postgres -v ON_ERROR_STOP=1 -q <"$ROOT/tests/browser/hostile/data.sql" >/dev/null
docker exec "$PG" psql -U postgres -v ON_ERROR_STOP=1 -q -c \
  "create role enlightenviz_reader login password 'reader-demo'; grant select on earthquake_events, hostile_events to enlightenviz_reader" >/dev/null

mkdir "$SCRATCH/boards"
cp "$ROOT/examples/earthquakes/board.yml" "$SCRATCH/boards/earthquakes.yml"
cp "$ROOT/tests/browser/hostile/board.yml" "$SCRATCH/boards/hostile.yml"
docker run -d --name "$SERVER" --network "$NET" --read-only --cap-drop ALL \
  -v "$SCRATCH/boards:/boards:ro" \
  -e ENLIGHTENVIZ_DATABASE_URL="postgres://enlightenviz_reader:reader-demo@$PG/postgres" \
  -e ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE=true \
  enlightenviz-image:browser-test >/dev/null

browser() {
  docker run --rm --network "$NET" "$@"
}
tries=0
until browser enlightenviz-browser:test node -e "fetch('http://$SERVER:8080/healthz').then((r) => process.exit(r.ok ? 0 : 1), () => process.exit(1))"; do
  tries=$((tries + 1))
  if [ "$tries" -gt 30 ]; then
    docker logs "$SERVER" >&2
    echo "the server did not start" >&2
    exit 1
  fi
  sleep 1
done

case "$MODE" in
  smoke)
    browser enlightenviz-browser:test node smoke.mjs "http://$SERVER:8080" earthquakes hostile
    ;;
  capture)
    mkdir -p "$OUT"
    browser -v "$(cd "$OUT" && pwd):/out" enlightenviz-browser:test node capture.mjs "http://$SERVER:8080" /out earthquakes
    ;;
  *)
    echo "unknown mode $MODE" >&2
    exit 2
    ;;
esac
