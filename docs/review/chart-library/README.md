# Chart library: Vega-Lite or Apache ECharts

For the operator's choice, by eye ([design](../../design.md), sections 2 and 9).
The same board, [`earthquakes-vega`](../../../examples/earthquakes-vega/board.yml)
and [`earthquakes-echarts`](../../../examples/earthquakes-echarts/board.yml), is
drawn by each library under the same house theme, from the same queries over the
same invented data. This page states facts; it does not pick a library. Both
implementations stay on the branch until the choice is made, and the loser is
then removed (its adapter in `web/`, its files in `vendor/`, its key in the
board format, its example).

All measurements are from one Mac (aarch64) under Docker, with Chromium
headless inside a container on a Docker network with no route out, nothing
throttled. The screenshots use the Noto Sans font installed in that container,
not the visitor's `system-ui`.

## What to look at

| | Vega-Lite | ECharts |
|---|---|---|
| Light, 1440 px | [image](earthquakes-vega-light-1440.png) | [image](earthquakes-echarts-light-1440.png) |
| Dark, 1440 px | [image](earthquakes-vega-dark-1440.png) | [image](earthquakes-echarts-dark-1440.png) |
| Light, 375 px | [image](earthquakes-vega-light-375.png) | [image](earthquakes-echarts-light-375.png) |
| Dark, 375 px | [image](earthquakes-vega-dark-375.png) | [image](earthquakes-echarts-dark-375.png) |
| Transition after a filter change (all classes to 5 and above) | [8 frames](transition-earthquakes-vega.png) | [8 frames](transition-earthquakes-echarts.png) |

A transition image is eight frames of the first chart's card, read left to right
and then the second row, about 60 ms apart (Vega-Lite at +0 +50 +133 +183 +233
+317 +367 +367 ms, ECharts at +0 +67 +117 +183 +233 +317 +367 +383 ms: the
browser delivers a frame about every 17 to 33 ms and the closest one to each
60 ms step is kept). The first two frames are the previous data at reduced
opacity while the request is out; the animation starts at the third.

## Comparison

| | Vega-Lite | ECharts |
|---|---|---|
| **Script of the page**, raw / gzip | 826,986 / 278,726 bytes | 1,159,692 / 381,894 bytes |
| of which the library | Vega 521,123 + Vega-Lite 257,954 + interpreter 9,132 = 788,209 / 264,467 | 1,121,883 / 368,202 (`echarts.min.js`, the full build: the map needs `geo`, which the smaller builds lack) |
| of which the page's own code (shared) | 27,831 / 10,071 | 27,831 / 10,071 |
| The library's adapter | `web/chart-vega.js`, 300 lines | `web/chart-echarts.js`, 256 lines |
| World outline (both) | 65,550 / 23,439 bytes, `world-110m.json` (under the 150 KB limit) | same file |
| **What the strict CSP required** | Vega compiles expressions to code with `Function`, which the policy forbids. It has a mode for that: the spec is parsed with `ast: true` and the View gets `vega-interpreter`, which evaluates expression trees. The interpreter is a 9 KB ES module that imports `vega-util` by bare name, which a browser cannot resolve without a bundler or an inline import map; its first line is replaced by a read from `globalThis.vega` (`vendor/README.md`). Cost, measured on the stacked chart (120 rows) with the CSP bypassed for the comparison: 3.2 ms per data update with the interpreter against 1.7 ms compiled; first draw 5.0 against 7.1 ms. Nothing else was needed. | Nothing. No violation in the smoke test. The canvas renderer draws no inline style, and ECharts sets its tooltip's style through the DOM's style object, which the policy allows. |
| **Time from navigation to the first chart**, median of 8 runs | cold (browser cache off) 125 ms; warm 83 ms | cold 150 ms; warm 93 ms |
| **Transition after a filter change** | Vega-Lite has none. `chart-vega.js` interpolates the numeric fields of the rows from the old frame to the new one and Vega redraws at every step: about 40 lines for the interpolation (`plan`, `mix`, `frame`), about 25 for the driver, 12 for the rule that geographic positions hold still. The scales and axes are recomputed each frame, so the y axis rescales smoothly with the bars; a category that leaves (a day) is shrunk to zero and dropped at the end, and the x axis then closes up at once. | Native: `setOption` with new rows animates by itself (300 ms, cubic-out, set in the theme). Zero lines written. In the frames, the y-axis labels jump and overlap for a frame (frame 4) while the bars move. |
| **Hover** | A tooltip at the pointer; the handler and the box are written here (about 40 lines). The hovered mark is not highlighted (the spec would need a parameter). | A tooltip and a highlight of the hovered mark, native; the content is built here as DOM nodes through the `formatter`. |
| **Keyboard** | Arrow keys, Home, End step through the marks that carry a tooltip (120 in the stacked chart, one per segment) and show the tooltip with a ring around the mark; written here. | The same keys step through the x positions (30 days) and show the library's own tooltip and highlight, through `dispatchAction`. |
| **Legend** | Native; laid out in one row, in two columns under 480 px, which needs the chart to be compiled again when the card crosses that width (written here). | Native; shown by the adapter when two or more series are named. One row at every width. |
| **What `validate` can check in the specification** | Every `field` anywhere, against the query's columns and the names a transform creates with `as`. Cannot see: a created name without `as`, `fields` and `groupby` lists, a `field` that is not a string. A column with a dot in its name (`4.5`) is read by Vega as a path and breaks `fold` at run time; `validate` accepts it (the example avoids such names). The published Vega-Lite JSON Schema exists, but `validate` does not apply it. | `encode`, `dimensions` and `visualMap.dimension`. Cannot see: columns in a tooltip's `formatter` template, anything reached by position, series names, or whether an option key exists at all: ECharts has no published schema to check against. |
| **Lines of specification per chart** (key line and non-comment lines) | 19, 13 and 20: 52 in all | 9, 9 and 20: 38 in all |
| **Licence** | Vega, Vega-Lite and vega-interpreter: BSD-3-Clause | Apache-2.0 (the project's own licence), with a `NOTICE` file |
| **Resident memory of the server with the assets embedded** | the same binary holds both: 4.4 MiB after start, 4.55 MiB after 1,000 page loads (each a page, `vega.min.js`, `echarts.min.js`, the definition and a query), peak 5.2 MiB. Before the page, the first slice measured 2.3 MiB after start. The image went from 3.9 MB to 6.9 MB. | same binary |

## Things that could not be done, or look wrong

- **Bar thickness in Vega-Lite.** The house rule is "at most 24 px". Vega-Lite
  has no maximum: the example uses half of the band, which is under 24 px with
  six regions in a 320 px card, and would be thicker with two. ECharts has
  `barMaxWidth`.
- **Hit area of a point, 24 px.** Neither library tests a hit against more than
  the drawn symbol. Each map therefore draws a second, invisible, 24 px mark over
  every point (11 lines of specification in Vega-Lite, 8 in ECharts). In ECharts
  the highlight then falls on the invisible mark, so the visible one is not
  highlighted.
- **The map in ECharts.** ECharts stretches a map to its box, and a partial
  `setOption` of the box moves the map but not the points over it, so the adapter
  works out the proportional box itself and builds the chart again when the
  card's size changes the box. Vega-Lite fits a projection to its data by itself.
  Both draw longitude and latitude at one scale (equirectangular): Vega-Lite can
  draw better projections, and was kept to this one to compare like with like.
- **A time axis in ECharts** gives bars about half the width of Vega-Lite's
  unless the series say `barWidth: "90%"`, which the example does.
- **Ordered colours.** The brief's ramp is applied by `palette: ordered`
  (`docs/boards.md`): ECharts cannot take it from a theme otherwise. In Vega-Lite
  an `ordinal` colour already uses it.
- **Axis text contrast.** The decided muted colour `#898781` on the card
  surface `#fcfcfb` is about 3.5:1, under the 4.5:1 that small text needs. Left
  as decided; it is the axis labels and nothing the visitor needs to read to
  understand a value, which is in the tooltip, the table and the labels.
- **`tabular-nums` on axis ticks** is not available in a canvas chart: the
  libraries draw text with the browser's canvas API, which has no such setting.
  Tables have it.
- **Stacked segments** are rounded at the data end each (4 px), with the 2 px
  gap; a stack's inner segments are therefore rounded too.
- **A Vega-Lite warning in the console** ("Domains that should be unioned has
  conflicting sort properties") appeared while the ranked chart sorted its
  layered axis; the example now keeps the query's order (`sort: null`) and the
  smoke test sees no warning.
- **Not in this slice:** live updates, TLS to Postgres, dbt, a selection in one
  chart that filters another, and line and area charts in the page (their house
  styles are set in both adapters but no example draws them, so they are not
  tested).

## Reproduce

```sh
tests/browser/run.sh smoke                                # the checks, on the closed network
tests/browser/run.sh capture docs/review/chart-library    # these images and the timings
```

The measurements of the interpreter's cost and of the server's memory were made
by hand and are not in a script: the first by building the same compiled
Vega-Lite specification twice in the page with the CSP bypassed
(`Page.setBypassCSP`), once with `ast: true` and the interpreter, once without,
and timing 60 data updates each, four times alternating; the second by reading
`VmRSS` from `/proc/1/status` of the server container (`docker run --rm
--pid=container:<server> alpine`), after 1,000 loops of five requests.
