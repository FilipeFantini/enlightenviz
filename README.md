# enlightenviz

A small server that draws dashboards written as code: SQL and chart
descriptions in Git, served with filters and live updates from a read-only
database connection.

**Status: a walking skeleton.** The first slice works end to end for data and
does not draw anything yet. Read [`docs/design.md`](docs/design.md): what it is
for, what it will not do, its security model, and the criteria by which the
first version is judged or the project abandoned.

Maintained by one person, with AI agents writing the code.

## What works now

- **Boards as YAML files**, read into strict types: unknown fields are errors.
  The whole format is on one page, [`docs/boards.md`](docs/boards.md), and its
  JSON Schema is [`schema/board.schema.json`](schema/board.schema.json)
  (`enlightenviz schema`).
- **`enlightenviz validate <dir>`**: the YAML, the schema, the SQL parameters,
  which query each chart reads and duplicate ids, offline; with a database it
  also prepares every query and checks that each field a chart draws is a column
  its query returns. Every error has the file, the line, a rule id and what to
  change, as text or with `--format json`. A non-zero exit code on any error.
- **`enlightenviz serve`**: loads and validates every board, prepares every query,
  and answers `GET /api/boards/{board}/queries/{query}` with the rows as JSON and
  the column names, and `GET /healthz`. Any problem at start stops the start.
- **Safe by construction.** The browser names a query and gives typed values;
  every value is checked against its declared type, sent as text and cast by the
  rewritten SQL, never concatenated. Each query runs in a `READ ONLY`
  transaction under a statement timeout, on a bounded connection pool, under a
  row limit. A result over the limit is an error, not a short result. No SQL,
  connection string or database message reaches a client; the server logs the
  detail under an id that the client also receives.

Not yet: the page that draws charts, filters in the browser, live updates, other
data sources, dbt, login. A chart is validated and kept, not drawn.

## Try it

You need Docker and nothing else. This starts a throwaway Postgres, loads the
invented data of the example board, builds the image and serves the board.

```sh
docker network create enlightenviz-net
docker run -d --name enlightenviz-pg --network enlightenviz-net \
  -e POSTGRES_PASSWORD=choose-one postgres:18.7-alpine3.24
# wait a few seconds for it to start, then:
docker exec -i enlightenviz-pg psql -U postgres -v ON_ERROR_STOP=1 < examples/earthquakes/schema.sql
docker exec enlightenviz-pg psql -U postgres -c \
  "create role enlightenviz_reader login password 'reader-demo'; grant select on earthquakes_daily to enlightenviz_reader"

docker build -t enlightenviz-image:dev .
docker run -d --name enlightenviz-server --network enlightenviz-net -p 127.0.0.1:8080:8080 \
  --read-only --cap-drop ALL -v "$PWD/examples/earthquakes:/boards:ro" \
  -e ENLIGHTENVIZ_DATABASE_URL=postgres://enlightenviz_reader:reader-demo@enlightenviz-pg/postgres \
  -e ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE=true \
  enlightenviz-image:dev

curl 'http://127.0.0.1:8080/api/boards/earthquakes/queries/by_day?from=2024-01-29&regions=Andes'
curl http://127.0.0.1:8080/healthz
```

Clean up with `docker rm -f enlightenviz-server enlightenviz-pg`,
`docker network rm enlightenviz-net` and `docker rmi enlightenviz-image:dev`.

The role is the point: the server's database role should be able to read only
what the boards need. The read-only transaction is a second barrier, not the
first.

To check boards without serving them (add the database variables to also
prepare the SQL against a real database):

```sh
docker run --rm -v "$PWD/examples/earthquakes:/boards:ro" enlightenviz-image:dev validate /boards
```

## Settings

Every setting is a flag or an environment variable; the variable is shown. The
image sets `ENLIGHTENVIZ_BOARDS=/boards` and `ENLIGHTENVIZ_LISTEN=0.0.0.0:8080`.

| Flag | Variable | Default | Meaning |
|---|---|---|---|
| (none) | `ENLIGHTENVIZ_DATABASE_URL` | | The Postgres connection URL. There is no flag for it, because arguments are visible to every user of the machine. |
| `--database-url-file` | `ENLIGHTENVIZ_DATABASE_URL_FILE` | | A file that holds the URL (a mounted secret). Set this or the variable, not both. |
| `--allow-unencrypted-database` | `ENLIGHTENVIZ_ALLOW_UNENCRYPTED_DATABASE` | off | Required: this build cannot encrypt the connection (see known gaps). |
| `--boards` | `ENLIGHTENVIZ_BOARDS` | required | The directory of board files. |
| `--listen` | `ENLIGHTENVIZ_LISTEN` | `127.0.0.1:8080` | Address and port to listen on. |
| `--pool-size` | `ENLIGHTENVIZ_POOL_SIZE` | 4 | The most connections to Postgres, 1 to 64. |
| `--acquire-timeout-ms` | `ENLIGHTENVIZ_ACQUIRE_TIMEOUT_MS` | 2000 | How long a request waits for a free connection before it gets a 503. |
| `--statement-timeout-ms` | `ENLIGHTENVIZ_STATEMENT_TIMEOUT_MS` | 5000 | How long one statement may run before Postgres cancels it. |
| `--row-limit` | `ENLIGHTENVIZ_ROW_LIMIT` | 10000 | The most rows a query may return. |

`validate` takes the first three, and `--format text|json`. Commands:
`enlightenviz schema`, `enlightenviz validate <dir>`, `enlightenviz serve`.

## Known gaps

Each of these is a decision to take later, not an oversight.

- **No TLS to Postgres.** This slice has no TLS library, so the connection,
  password included, travels unencrypted. The server refuses to start unless
  `--allow-unencrypted-database` says that is meant. Use it only on a network you
  trust (a container network, a loopback). The design's default, TLS with the
  server's name verified, is still to come.
- **No page, no browser filters, no live updates.** See [the design](docs/design.md).
- **Rate limits, and slow clients, belong to the proxy in front.** The server
  bounds its database use (pool, timeouts, row limit) but accepts any number of
  client connections and has no header or body timeouts.
- **Boards are read once, at start.** Editing a board needs a restart.
- **The row limit is a `LIMIT`.** Postgres stops early for most plans; a plan that
  must build the whole result first (a set-returning function in `FROM`, a big
  sort) runs to completion or to the statement timeout, whichever is first.
- **`number` is `double precision`, and JSON numbers are doubles in JavaScript.**
  A `bigint` above 2^53 loses precision there; cast it to text in the SQL.
- **`validate` reads chart fields by a simple rule** (see
  [`docs/boards.md`](docs/boards.md#a-chart)); a field a transform creates without
  `as` is reported as unknown.
- **SQL is read assuming `standard_conforming_strings = on`**, the Postgres
  default since 9.1.
- **Unix only.** The server handles `SIGTERM` and `SIGINT` to stop cleanly.
- **No access log**: only errors and start and stop are logged, as `key=value`
  lines on standard error.

## Measured

On one Mac (aarch64) under Docker, with the example board and a pool of 4,
resident memory of the server was 2.3 MiB right after start and 2.7 MiB after 1,000
sequential and 200 concurrent requests, with a peak of 6.8 MiB during the
concurrent burst; the image is 3.9 MB. Design section 10 sets a ceiling of 64 MiB.
This is a measurement, not a CI gate yet.

## Development

Nothing is installed on the host: builds and tests run in containers. With the
Rust image named in the [`Dockerfile`](Dockerfile) (the same version as
[`rust-toolchain.toml`](rust-toolchain.toml)) and a Postgres reachable for the
tests:

```sh
RUST=rust:1.100.0-alpine3.24@sha256:de1e0058543a81ed7b046629167db587c67a94bdf37cce14eb27f0ec83baaffb
cargo() { docker run --rm --network enlightenviz-net -v "$PWD:/src" \
  -v enlightenviz-cargo:/usr/local/cargo/registry -v enlightenviz-target:/target \
  -v enlightenviz-rustup:/usr/local/rustup -e CARGO_TARGET_DIR=/target \
  -e ENLIGHTENVIZ_TEST_DATABASE_URL=postgres://postgres:choose-one@enlightenviz-pg:5432/postgres \
  -w /src "$RUST" cargo "$@"; }

cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The tests that need a database use `ENLIGHTENVIZ_TEST_DATABASE_URL`, the URL of a
superuser of a disposable database: the first of them creates the example table,
a scratch table and three throwaway roles there. When it is not set those tests
fail; they are not skipped. After a change to the format, regenerate the schema
(`enlightenviz schema > schema/board.schema.json`): a test fails while the
committed file differs from the generated one.

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the format
check, clippy with warnings as errors, the tests against a Postgres service,
`cargo deny` ([`deny.toml`](deny.toml): advisories, licenses, bans, sources), a
secret scan (gitleaks) and the image build. Its actions are pinned by commit.

The dependencies, and why each is there: `axum` (the HTTP server), `tokio` (the
async runtime, one thread), `tokio-postgres` and `deadpool-postgres` (the
Postgres client and its bounded pool), `serde`, `serde_json` and
`serde_path_to_error` (reading a board, and where in it an error is), `schemars`
(the JSON Schema, from the same types), `saphyr` and `saphyr-parser` (YAML with
line numbers), `clap` (flags and environment variables), `form_urlencoded` (the
query string).

## License

[Apache-2.0](LICENSE).
