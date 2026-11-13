# Writing a board

A board is one YAML file: the SQL a dashboard runs, the parameters that SQL
accepts, and the charts that draw its results. This page describes the format
completely; the JSON Schema of the same format is printed by
`enlightenviz schema` and committed as
[`schema/board.schema.json`](../schema/board.schema.json). A working example is
[`examples/earthquakes/board.yml`](../examples/earthquakes/board.yml).

What exists today is the format, `validate`, and the JSON route that answers a
query. There is no page yet: a chart is validated and kept, not drawn. See the
[README](../README.md#known-gaps).

## Files

- The server and `validate` read a directory. Every file directly inside it
  whose name ends in `.yml` or `.yaml` is a board, in name order. Other files,
  and sub-directories, are ignored. A directory with no board is an error.
- One board per file, one YAML document per file, at most 1 MiB, UTF-8.
- YAML 1.2. `yes`, `no` and `on` are text; `2024-01-01` is text, not a date. Keys
  must be strings, and duplicate keys are an error. Anchors, aliases (`&a`,
  `*a`) and tags (`!!str`) are not supported: write the value out.
- Unknown fields are errors everywhere, so a misspelt key is caught, not ignored.

## A board

| Field | Type | Required | Meaning |
|---|---|---|---|
| `board` | name | yes | The board's id. Part of every URL. Unique across the directory. |
| `title` | text | yes | The title shown above the board. |
| `text` | text | no | Text shown under the title. |
| `queries` | map of name to query | yes | The queries, by name. |
| `charts` | list of chart | yes | The charts, in order. At least one. |

A **name** (the board id and each query name) is 1 to 63 characters from
`A-Z a-z 0-9 _ -`, starting with a letter or `_`. A **parameter name** is the same
without `-`, because it is written in SQL.

## A query

| Field | Type | Required | Meaning |
|---|---|---|---|
| `sql` | text | yes | One SQL statement. See "The SQL". |
| `params` | map of name to parameter | no | The typed parameters the SQL uses. |

### The SQL

- Write one `SELECT`, or one `WITH ... SELECT` whose `WITH` parts only read. A
  trailing `;` is allowed; a `;` followed by more SQL is an error. `INSERT`,
  `UPDATE`, `DELETE` and any data-modifying `WITH` are refused when the server
  starts (and by `validate` against a database), and every query also runs in a
  `READ ONLY` transaction, so a function that writes is refused when it runs.
- Write a parameter as `$name`. Every `$name` must be declared under `params`,
  and every declared parameter must be used. Do not write `$1`: that is an error.
  The same `$name` may appear more than once.
- A `$name` inside a single-quoted string (`'...'`, and `E'...'`), a
  double-quoted identifier (`"..."`), a `--` comment, a `/* */` comment (they
  nest) or a dollar-quoted string (`$$...$$`, `$tag$...$tag$`) is plain text, not a
  parameter. A `$` inside a word (`a$b`) is part of the word.
- A parameter stands for a value of its declared type, so the SQL can compare it
  directly: `where day >= $from`, `region = any($regions)`. A cast after it
  (`$from::date`) is harmless.
- Every output column needs its own name (`as`), and names must not repeat: a
  row is sent as a JSON object. The order of the rows is the order the SQL gives,
  so write `order by`.
- Values reach the client as Postgres writes them in JSON: integers and numerics
  as numbers, text as strings, dates and timestamps as ISO 8601 strings, arrays as
  arrays, `json` columns as nested JSON. A `bigint` above 2^53 loses precision in
  any JavaScript that reads it; cast it to `text` in the SQL if that matters.
- A result with more rows than the server's row limit (10,000 by default) is an
  error, never a shorter result. Narrow it with a parameter or a `limit`.

## A parameter

```yaml
params:
  from: {type: date, default: "2024-01-01"}
  regions: {type: "list<text>", max_items: 10}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | one of the types below | yes | What a value must be. |
| `default` | a value of the type | no | Used when a request gives none. Without it the parameter is required. |
| `max_items` | integer, 1 to 1000 | no | Lists only: the most values a request may give. Default 100. |

| `type` | A value must be | Postgres type | `default` is written as |
|---|---|---|---|
| `text` | any text without a NUL character | `text` | a quoted string |
| `integer` | a whole number that fits in 64 bits, e.g. `42`, `-7` | `bigint` | a YAML integer |
| `number` | a finite number, e.g. `1.5`, `1e3` (no `NaN`, no infinity) | `double precision` | a YAML number |
| `boolean` | exactly `true` or `false` | `boolean` | `true` or `false` |
| `date` | `YYYY-MM-DD`, years 0001 to 9999, a real day | `date` | a quoted string |
| `list<text>`, `list<integer>`, `list<number>`, `list<boolean>`, `list<date>` | items of that type | the type's array | a YAML list; `[]` is allowed |

A type with `<` in it must be quoted in YAML. A default is checked when the board
loads: write `default: "2024-02-30"` and `validate` says the date does not exist.
Quote text and dates; a bare `default: 5` is a number, not text.

## A chart

| Field | Type | Required | Meaning |
|---|---|---|---|
| `title` | text | yes | The title of the chart. |
| `query` | text | yes | The name of a query of this board. |
| `vega_lite` | map | yes | A [Vega-Lite](https://vega.github.io/vega-lite/) specification. |

The specification is kept as written. `validate` reads two things from it. Every
`"field": "<name>"` anywhere in it must be a column the query returns, or a name a
transform creates with `as` (`{calculate: "...", as: double}`, `{fold: [...], as:
[key, value]}`). A field with a path (`a.b`, `a[0]`) is accepted when its first
part is a column; `\.` writes a literal dot. Limits: a name a transform creates
without writing `as` (such as an aggregate's default name) is reported as
unknown, so write `as`; the plural `fields` and `groupby` lists are not read; a
`field` that is not a string is skipped. This check needs a database; without one
it is skipped, and `validate` says so.

## Running a query

```
GET /api/boards/{board}/queries/{query}?name=value&name=value
```

Parameters come from the query string. A list parameter repeats its key
(`regions=Andes&regions=Cascadia`). A parameter the request omits takes its
default; one without a default is required. A scalar given twice, a name the query
does not declare, and a value that is not of its type are 400s. Percent-encoding
and `+` for a space work as in any query string; bytes that are not UTF-8 are
refused.

The answer is `200` with

```json
{"columns": ["day", "events"], "rows": [{"day": "2024-01-05", "events": 4}]}
```

`columns` are the output column names in order; `rows` are objects. The answer
carries `Cache-Control: no-store`. An error is

```json
{"error": {"id": "6af6f5d4-e", "code": "bad_request", "message": "parameter `from` must be a date written YYYY-MM-DD"}}
```

The `id` is also written, with the detail, in the server's log, so an operator can
find the cause. The client never gets SQL, a table name or a database message.

| Status | `code` | When |
|---|---|---|
| 400 | `bad_request` | A parameter is unknown, missing, repeated, or not of its type. |
| 404 | `not_found` | The board or the query does not exist. |
| 422 | `row_limit` | The result has more rows than the row limit. |
| 500 | `internal` | The database refused or failed the statement. |
| 503 | `busy` | Every connection of the pool stayed busy for the whole acquire timeout. |
| 503 | `unavailable` | The database cannot be reached. |
| 504 | `query_timeout` | The statement ran past the statement timeout and was cancelled. |

`GET /healthz` answers `200 ok` while the process is up, and touches no database.

## Checking boards

```
enlightenviz validate <dir> [--format text|json]
```

Without a database it checks the YAML, the shape, the parameters, the SQL and
which query each chart reads. With one (`ENLIGHTENVIZ_DATABASE_URL` and
`--allow-unencrypted-database`, see the README) it also prepares every query in a
read-only transaction and checks the charts' fields against the columns. Every
problem is reported, not only the first; each has the file, the line where it can
be known, a rule id, a message and what to change. Exit code `0`: nothing wrong;
`1`: a board has a problem; `2`: the run itself failed (no directory, bad
configuration). `serve` runs the same checks and does not start if any fails.

The JSON form:

```json
{"ok": false, "database_checked": true, "diagnostics": [
  {"file": "boards/a.yml", "line": 22, "rule": "chart-unknown-field",
   "message": "...", "fix": "..."}]}
```

A line points at the key where the problem is; a problem inside a SQL text is
reported at the `sql` key, with the line within the query in the message.

| Rule | Meaning |
|---|---|
| `yaml-syntax` | The file is not valid YAML. |
| `yaml-unsupported` | An anchor, alias, tag, non-string key, second document, or nesting deeper than 100. |
| `schema` | A field is unknown, missing, or of the wrong type. |
| `name-invalid` | A board id, query name or parameter name has characters it may not. |
| `board-duplicate` | Two files use the same board id. |
| `board-no-charts` | `charts` is empty. |
| `chart-unknown-query` | A chart reads a query the board does not declare. |
| `param-default-invalid` | A default is not a valid value of its type. |
| `param-max-items-invalid` | `max_items` on a non-list, or outside 1 to 1000. |
| `sql-undeclared-param` | The SQL uses a `$name` that is not declared. |
| `sql-unused-param` | A declared parameter is never used. |
| `sql-positional-param` | The SQL uses `$1`. |
| `sql-unterminated` | A string, identifier, comment or dollar quote never closes. |
| `sql-multiple-statements` | A `;` is followed by more SQL. |
| `sql-errors-truncated` | A query has more than 50 repeated SQL problems (undeclared names, `$1`); the first 50 are listed and this says how many were not. Fix them and run again. |
| `sql-empty` | The SQL holds only whitespace and comments. |
| `boards-dir-unreadable`, `boards-dir-empty`, `file-unreadable`, `file-too-large` | The directory or a file cannot be used. |
| `database-unreachable` | (With a database) no connection could be made. |
| `sql-prepare-failed` | (With a database) Postgres cannot prepare the query. |
| `sql-prepare-timeout` | (With a database) preparing ran past the statement timeout. |
| `sql-not-a-select` | (With a database) the statement is not a read-only `SELECT`. |
| `query-column-duplicate` | (With a database) two output columns have one name. |
| `chart-unknown-field` | (With a database) a chart draws a field that is no column and no `as`. |

## Not in this format yet

No `refresh` or live updates, no filters bound to parameters, no choice of data
source, no cache settings, no `ref()` to dbt models. They are later slices of
[the design](design.md); a board that uses one is rejected as an unknown field.
