# Design — enlightenviz

**Status:** approved by the operator on 2026-11-13. A change to sections 3, 4
or 10 is a new decision of the operator's, not an edit.

**Date:** 2026-11-12

## 1. What this is

A small server that draws dashboards written as code. A dashboard is a text
file in Git: SQL the author wrote, and a description of the charts. The server
runs that SQL against a database as a read-only user, and a visitor's browser
draws the charts, with filters, and with updates pushed as new data arrives.

It exists because of one gap, found while building a data platform on an 8 GB
node ([Lagom](https://github.com/FilipeFantini/lagom), ADR-0041): no tool found
on 2026-11-12 was at once

1. dashboards as code,
2. batch and streaming,
3. small enough to sit beside the data platform it serves,
4. safe to expose to the internet,
5. open source.

Static tools (Evidence, a `dct render` page) meet 1, 3 and 5 and have no
streaming. Server BI tools (Lightdash, Superset, Metabase) meet 4 and 5, keep a
dashboard's state in their own database, refresh on an interval, and cost 600
MB to 1 GB or more. Grafana meets 2 to 5 as an observability product, not a
data product. This list is from documentation and one spike, not from a full
survey; section 11 says what reverses the decision.

## 2. Principles

- **A dashboard is a declaration, not a program.** It can be validated,
  compared and reviewed without being run.
- **The browser never sends SQL.** It names a query the board declares and
  gives typed parameters.
- **Small is a requirement.** Resident memory is measured in CI and has a
  ceiling (section 10).
- **Nothing leaves.** The server and the page make no request to any host the
  operator did not configure. No telemetry.
- **Standards over inventions.** Vega-Lite for charts, the Postgres wire
  protocol for data, OpenID Connect for login, dbt's artifacts for lineage.
  This project is the thin layer between them.
- **Written to be driven by an agent.** Every rule a board must follow is in a
  published schema, and every failure names the file, the line and the fix.
- **Fail loudly.** A query that fails or returns nothing is an error state the
  visitor sees and the build catches, never an empty chart that looks like a
  zero.

## 3. Scope of v0

The first version does exactly this, and is judged on it (section 10).

- **Boards as files.** One YAML file per board: named queries, their typed
  parameters, charts as Vega-Lite specifications bound to a query, filters
  bound to parameters, text.
- **One data source: Postgres**, through a read-only role.
- **A server** with four kinds of route: a board's page, a query's result as
  JSON, a query's updates as a stream, and health.
- **Filters in the page:** a filter changes a parameter, the browser asks for
  the query again.
- **Live updates:** a chart may declare an interval or a Postgres notification
  channel; the server pushes new results over Server-Sent Events (one HTTP
  response kept open, which every browser and proxy already supports).
- **A command line:** `validate` (schema, SQL parameters, chart fields against
  the query's columns), `render` (a board's data as JSON, for tests and for
  agents), `serve`.
- **One container image**, running as a non-root user on a read-only file
  system.

## 4. What it will not do

Written down so that a request for any of these is a new decision, not a
drift.

- No visual editor. A board is edited as text.
- No users, groups or passwords of its own. Login, when it comes, is delegated
  (section 7).
- No ad-hoc SQL from a visitor, and no SQL console.
- No semantic layer. Metrics are defined upstream, in dbt.
- No chart grammar of its own. If Vega-Lite cannot draw it, the board does not
  have it.
- No transformation, scheduling or caching of data at rest. It reads what is
  served to it.
- No write to any data source, ever.
- No connector written here for a source that a standard already reaches.
- No plugin system in v0.

## 5. How it works

```
board.yml  --validate-->  server (read-only role)  -->  Postgres
                               |  page, JSON, event stream
                               v
                           browser: Vega-Lite draws; filters set parameters
```

1. At start, the server loads the boards and prepares each declared query. A
   board that fails validation stops the start.
2. A visitor opens a board. The server sends a page with the chart
   specifications and no data.
3. The browser asks for each query by name, with the filters' values. The
   server checks each value against the parameter's declared type and range,
   binds it to the prepared statement, runs it under a statement timeout and a
   row limit, and answers with JSON and a cache header.
4. For a live chart, the browser opens an event stream; the server runs the
   query again on the declared trigger and pushes the result if it changed.

State: the server holds none between requests except its connection pool and
its open streams. Boards come from the image or a mounted directory.

**A sketch of a board,** to be settled by the schema in v0, not by this
document:

```yaml
board: earthquakes
title: Earthquakes, last 30 days
source: serving
queries:
  by_day:
    sql: |
      select day, region, events
      from earthquakes_daily
      where day >= $from and region = any($regions)
    params:
      from:    {type: date, default: "-30d"}
      regions: {type: "list<text>", from: regions}
    refresh: {every: 60s}
charts:
  - title: Events a day
    query: by_day
    vega_lite:
      mark: bar
      encoding:
        x: {field: day, type: temporal}
        y: {field: events, type: quantitative}
```

## 6. Built for agents

- **A JSON Schema of the board format,** published with each release.
- **`validate` is complete:** it checks the schema, that every parameter a
  query uses is declared, and that every field a chart draws is a column the
  query returns. Against a live source it also prepares each statement.
- **Errors are structured:** file, line, rule, and what to change; as text and
  as JSON.
- **`render` is deterministic:** the same data gives the same output, so an
  agent, or a test, can compare a board's numbers with their source.
- **One page of documentation an agent can read whole.**

## 7. dbt

dbt Core is the standard this is built to sit beside. Only its open artifacts
are read; nothing depends on a dbt engine.

- **`ref()` in a board's SQL** is resolved from `manifest.json`.
- **Contracts:** where a model declares its columns and types, `validate`
  checks a board's queries against them. A renamed column fails a build, not a
  page.
- **Exposures:** the tool writes a dbt exposure for each board, so a board
  appears in dbt's lineage and in any orchestrator that reads it.
- **Freshness:** a board can show, and `validate` can test, the age of what it
  reads.

All four are optional: a board over a plain Postgres database needs no dbt.
**None of them is in v0:** its criteria (section 10) are met over plain
Postgres tables, and each one adds a way to fail before the server is proven.
`ref()` and contracts are the first work after v0.

## 8. Security

### What is protected, and from whom

| Asset | Threat | Answer |
|---|---|---|
| The database behind the boards | A visitor runs SQL of their own | The browser sends a query's name and typed values. Values are bound, never concatenated. |
| | A visitor exhausts it | Statement timeout, row limit, a bounded connection pool, a limit on open streams; rate limits belong to the proxy in front. |
| | The server is taken over | Its role is read-only and sees only what a board needs; the transaction is read-only as well. |
| Data a visitor may not see | A private board is opened without login | Login is delegated to OpenID Connect; a board declares who may open it. Not in v0, which serves public data only and says so. |
| | One viewer reads another's rows | The server passes the identity to the database, and the database's row-level rules decide. Not in v0. |
| The source's credentials | They leak through a file, a log or an error | Read from the environment or a file, never from a board; never printed; certificate authentication to Postgres is supported so that no password exists. |
| The operator's privacy | The tool reports home | No telemetry, no update check, no font or script from another host. A test in CI runs the server with the network closed. |
| The user of a release | A tampered binary or dependency | Signed images and binaries, a bill of materials with each release, dependencies audited in CI, a minimal base image. |

### Defaults

Secure settings are the defaults and weakening one is explicit: read-only
transactions, TLS to the source with the server's name verified, a strict
content security policy on every page, no directory listing, no debug route in
a release build.

### Reports

A `SECURITY.md` says how to report a vulnerability privately before the first
release.

## 9. Technology

Candidates; each is confirmed or replaced by the first working slice, and the
choice recorded.

| Piece | Candidate | Why |
|---|---|---|
| Server | Rust | One static binary, no runtime, resident memory in the tens of MiB, memory safety checked by the compiler, which also checks an agent's code before it runs. Go is the alternative if Rust slows the work. |
| Charts | Vega-Lite, drawn in the browser | An open, declarative grammar with a published schema; agents write it well. Served from the binary, not from a CDN. |
| First source | Postgres wire protocol | Covers Postgres and the databases that speak it. |
| Later sources | ADBC, an embedded DuckDB | Open standards that reach warehouses and files without a connector written here. Not in v0. |
| Live updates | Server-Sent Events | Plain HTTP; no second protocol for proxies to carry. |
| License | Apache-2.0 | Permissive, with a patent grant. |

## 10. Done, and abandoned

**v0 is done when,** in Lagom's staging environment:

1. the earthquakes board is served from the `serving` database with a region
   filter that changes its numbers;
2. one chart updates without a reload when a row is inserted;
3. `validate` fails on a board that names a column its query does not return;
4. resident memory stays under **64 MiB** after 1,000 page views, with no
   growth between the 100th and the 1,000th;
5. the server holds no more connections than its pool's limit after those
   views;
6. the closed-network test passes.

**The project is abandoned, and Lagom returns to its static page, when** any of
these is true:

- v0 is not done by **2026-12-20**;
- criterion 4 or 5 cannot be met without a redesign;
- an existing tool is found that covers the five requirements of section 1.

Abandoning it is a recorded decision, not a failure to hide.

## 11. Risks

- **Building a tool instead of the product.** Section 4 and section 10 are the
  limits.
- **A front end is long work:** small screens, accessibility, empty and error
  states. v0 does them plainly, and they are reviewed by eye.
- **One maintainer.** A security fix depends on one person and their agents.
  Stated in the README until it stops being true.
- **Vega-Lite's weight in the browser** (about 1 MB of script, to be measured)
  on a slow connection.
- **Streaming at scale is untested.** v0 targets few events at high frequency
  and tens of viewers, not thousands of open streams.

## 12. Settled

- **The name,** 2026-11-12: `enlightenviz`. On that day the name was free on
  crates.io, PyPI, npm and GitHub, and `enlightenviz.com` and `.io` were
  unregistered. `enlighten` alone was not: the domain `enlighten.me`, the PyPI
  and npm packages, and a solar monitoring product already use it.
- **Public from the start,** 2026-11-12: the project is open source and so is
  its design. The license is Apache-2.0.
- **The abandonment date,** 2026-11-13: 2026-12-20 (section 10).
- **dbt in v0,** 2026-11-13: none (section 7).
- **Rust,** 2026-11-13. Go replaces it only by a recorded decision, if the
  first slice shows Rust costs more than it gives.
