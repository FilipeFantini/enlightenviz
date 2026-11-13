# Writing a board

A board is one YAML file: the SQL a dashboard runs, the parameters that SQL
accepts, the filters that set those parameters, and the stats and charts that
show the results. This page describes the format
completely; the JSON Schema of the same format is printed by
`enlightenviz schema` and committed as
[`schema/board.schema.json`](../schema/board.schema.json). Working examples are
[`examples/earthquakes-vega/board.yml`](../examples/earthquakes-vega/board.yml)
and
[`examples/earthquakes-echarts/board.yml`](../examples/earthquakes-echarts/board.yml):
the same board, with the charts written for each of the two chart libraries
under comparison.

What exists today is the format, `validate`, the JSON route that answers a
query, and the page that draws a board in the browser (see "The page"). There
are no live updates yet. See the [README](../README.md#known-gaps).

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
| `filters` | list of filter | no | The controls above the board, in order. See "A filter". |
| `stats` | list of stat | no | The figures above the charts, in order. See "A stat". |
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

## A filter

```yaml
filters:
  - {label: From, param: from, control: date}
  - {label: Regions, param: regions, control: multiselect,
     options_from: {query: regions, value: value, label: label}}
  - label: Magnitude class
    param: min_magnitude
    control: select
    options:
      - {value: 0, label: All classes}
      - {value: 4, label: "4 and above"}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `label` | text | yes | Shown with the control. |
| `param` | parameter name | yes | The parameter the filter sets. |
| `control` | `select`, `multiselect` or `date` | yes | How it is drawn. It must fit the parameter's type (below). |
| `options` | list of `{value, label}` | for `select` and `multiselect` | A fixed list. `value` is written as the parameter's type: a quoted string for text, a number for integer and number, `true` or `false` for boolean. |
| `options_from` | `{query, value, label}` | for `select` and `multiselect` | Take the options from a query of this board: `value` and `label` are two of its columns. The query is run once, without parameters, so every parameter it declares needs a default. |

Give `options` or `options_from`, not both; a `date` filter has neither.

A filter sets the parameter of its name **in every query that declares it**: the
page asks again only the queries that declare it. So those declarations must
agree: the same type, the same default and the same `max_items`, and the
default must exist (it is the filter's starting value). A filter whose
parameter no query declares, and two filters for one parameter, are errors.

| `control` | Fits a parameter of type |
|---|---|
| `select` | `text`, `integer`, `number`, `boolean` |
| `multiselect` | any list type |
| `date` | `date` |

The state of the filters is in the URL's query string (`?min_magnitude=5&regions=Andes&regions=Tonga`),
and only what differs from the defaults is written, so a link reproduces the
view. A multiselect always keeps at least one choice: an empty list cannot be
sent in a query string, and would silently fall back to the default.

## A stat

```yaml
stats:
  - {label: Events, query: summary, value: events, format: integer, hero: true}
  - {label: Strongest, query: summary, value: strongest, format: one_decimal}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `label` | text | yes | Shown with the number. |
| `query` | text | yes | A query of this board. It must return exactly one row; any other count is an error shown in the tile. |
| `value` | text | yes | The column that holds the number. A `null` is shown as "No data". |
| `format` | `integer`, `one_decimal` or `compact` | no | `12,345`, `5.4`, `12.3K`. Without it: up to two decimals. |
| `hero` | boolean | no | The headline figure, drawn larger. At most one stat. |

## A chart

| Field | Type | Required | Meaning |
|---|---|---|---|
| `title` | text | yes | The title of the chart. |
| `query` | text | yes | The name of a query of this board. |
| `vega_lite` | map | one of the two | A [Vega-Lite](https://vega.github.io/vega-lite/) specification. |
| `echarts` | map | one of the two | An [Apache ECharts](https://echarts.apache.org/) option. |
| `palette` | `categorical` or `ordered` | no | Where the series take their colours from. See below. |

*During the comparison of the two libraries a chart has either key, exactly
one. Which library stays is decided by the operator, by eye; the other key is
then removed.*

The specification is kept as written, and **never carries data or a URL**: the
page adds the rows of the chart's query itself. `validate` refuses, anywhere in
a specification, the keys `data`, `datasets`, `dataset`, `url` and `href`, and
any string that starts `http://`, `https://`, `//` or `image://` (a symbol or
a link that would make the browser request something). The one static dataset
is the world outline (Natural Earth 110m, without Antarctica), which the page
provides under the reserved name `world`: in Vega-Lite a layer may say
`data: {name: world}` (that exact object, nothing else), and in ECharts a
`geo` or a series may say `map: world`. The page draws the outline with
longitude and latitude at one scale (equirectangular) in both libraries.

The page sizes the chart to its card, so do not write `width`, `height` or
`autosize`. It also sets the house theme (colours, fonts, gridlines, bar
thickness and corners, point sizes, tooltip, animation) underneath what the
specification says, so a specification that says nothing about style looks
finished, and one that says something wins.

**`palette`.** A chart's series take the eight categorical colours, in their
fixed order, which is what `categorical` (the default) means. `ordered` is for
classes that have an order, such as magnitude classes: the series take the
four-step blue ramp, from weakest to strongest, in the order they are drawn.
A colour is chosen by position in a list the author writes (a `scale.domain` in
Vega-Lite, the order of `series` in ECharts), not by the rows that happen to be
present, so a filter that removes a category does not repaint the others. In
Vega-Lite an `ordinal` colour already uses the ramp, so `palette` matters there
only for `nominal` colours; in ECharts it is the only way to ask for the ramp
without writing a colour that would be wrong in the other colour scheme.

### What `validate` checks in a specification

It reads two kinds of thing, both only with a database (it needs the query's
columns; without one the check is skipped, and `validate` says so).

**Vega-Lite.** Every `"field": "<name>"` anywhere in the specification must be a
column the query returns, or a name a transform creates with `as`
(`{calculate: "...", as: double}`, `{fold: [...], as: [key, value]}`). A field
with a path (`a.b`, `a[0]`) is accepted when its first part is a column; `\.`
writes a literal dot. Limits: a name a transform creates without writing `as`
(such as an aggregate's default name) is reported as unknown, so write `as`; the
plural `fields` and `groupby` lists are not read; a `field` that is not a
string is skipped. A column whose name contains a dot (`4.5`) is read by Vega
as a path: avoid such names, or write `\.` in the specification.

**ECharts.** ECharts has no `field` key. `validate` reads the places where an
option names a column of the rows: every value in `encode` (a name or a list of
names; a number is a position and is skipped), every name in `dimensions` (a
string, or an object's `name`), and `dimension` inside `visualMap`. What it
**cannot see**: a column written in a tooltip's `formatter` template
(`{@name}`), `series.name` and other text, a column reached by position, and
anything the page adds. A misspelt name there draws nothing or the wrong thing,
and `validate` stays silent. The Vega-Lite check is deeper, because a
Vega-Lite specification is a closed grammar and an ECharts option is not.

## The page

```
GET /                       the list of boards
GET /boards/{board}         the board's page
GET /api/boards/{board}     the board's definition, as JSON
GET /assets/{hash}/{name}   the scripts, styles and libraries, embedded in the binary
```

The page is a shell with no data and no inline script or style. Its script asks
`/api/boards/{board}` for the definition (title, text, filters with their type
and default and options, stats, charts with their specification, and each
query's parameters), builds the filter row and the cards, and asks each query
for its rows with the filters' values. A change of filter asks again only the
queries that declare its parameter; a chart keeps its previous frame at reduced
opacity while it waits, then animates to the new data (about 300 ms), or does
not animate when the visitor asks for reduced motion.

Every HTML answer carries
`Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`,
`X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`. A library
that needs a weaker policy is not used. Assets live at `/assets/{hash}/{name}`,
where `hash` is computed from all of them at start, and are cached for a year
(`immutable`); a new binary has a new hash.

A query that fails shows its message and its id in that chart's card; a query
with no rows says so. The page is light or dark with the system, and a
button wins either way. Text from data is put in the page as text, never as
markup. In the table view of a chart the first 500 rows are listed.

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
read-only transaction and checks that the columns that charts, stats and filters name are columns the queries return. Every
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
| `chart-spec-key` | A chart has neither `vega_lite` nor `echarts`, or both. |
| `chart-spec-forbidden` | A specification carries data or a link: `data`, `datasets`, `dataset`, `url`, `href`, or a string that starts `http://`, `https://`, `//`, `image://`. |
| `filter-unknown-param` | A filter drives a parameter no query declares. |
| `filter-duplicate` | Two filters drive one parameter. |
| `filter-param-conflict` | The queries a filter drives declare its parameter with different types, defaults or `max_items`. |
| `filter-param-required` | A parameter a filter drives has no default in some query. |
| `filter-control-type` | The control does not fit the parameter's type. |
| `filter-options-invalid` | The options are missing, doubled, empty, on a `date` filter, not of the parameter's type, do not include the default, or `options_from` names an unknown query or one that needs a parameter without a default. |
| `stat-unknown-query` | A stat reads a query the board does not declare. |
| `stat-hero-multiple` | More than one stat is `hero`. |
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
| `chart-unknown-field` | (With a database) a chart draws a field that is no column and no `as` (Vega-Lite), or names a column in `encode`, `dimensions` or `visualMap` that the query does not return (ECharts). |
| `stat-unknown-column` | (With a database) a stat's `value` is not a column of its query. |
| `filter-unknown-column` | (With a database) an `options_from` `value` or `label` is not a column of its query. |

## Not in this format yet

No `refresh` or live updates, no choice of data source, no cache settings, no
`ref()` to dbt models, no selection in one chart that filters another, no
layout control (the stats come first, then the charts in order; a last chart
alone on its row takes the whole row). They are later slices of
[the design](design.md); a board that uses one is rejected as an unknown field.
