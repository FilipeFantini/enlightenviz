// Draws a chart with Vega-Lite, under the house theme.
//
// What is native to Vega and Vega-Lite: the grammar, the compiler, the scales,
// axes and legends, the canvas drawing, and the data flow. What is written
// here, because the libraries do not have it: the house theme (as a Vega-Lite
// config, below the author's own), the rows and the world outline, the
// tooltip (Vega calls a handler; the box is built here, with textContent),
// keyboard stepping through the marks, and the animation of a change of data.
// Vega has no transitions: `tween` below interpolates the numeric fields of
// the rows from the old frame to the new one, and Vega redraws at every step.
//
// The page's content security policy forbids code built from text. Vega's
// expression compiler builds code from text, so the spec is parsed with
// `ast: true` and the View gets vega-interpreter, which evaluates the
// expression trees without compiling them.

import { expressionInterpreter } from "./vega-interpreter.mjs";
import { formatCell } from "./format.js";
import { tooltipContent } from "./tooltip.js";
import { loadWorld, usesWorld } from "./world.js";

const DURATION = 300;

// Under this width the legend is laid out in two columns, so that it never runs
// past the card. Vega-Lite fixes the legend's layout when the spec is compiled,
// so crossing the width means compiling again (`resize` says so).
const COMPACT = 480;

function houseConfig(theme, palette, compact) {
  const blue = theme.categorical[0];
  return {
    background: null,
    font: theme.font,
    view: { stroke: null },
    scale: { maxBandSize: 24, bandPaddingInner: 0.25 },
    axis: {
      grid: false,
      ticks: false,
      domainColor: theme.axis,
      domainWidth: 1,
      gridColor: theme.grid,
      gridWidth: 1,
      labelColor: theme.muted,
      labelFont: theme.font,
      labelFontSize: 12,
      labelPadding: 6,
      titleColor: theme.ink2,
      titleFont: theme.font,
      titleFontSize: 12,
      titleFontWeight: "normal",
    },
    axisQuantitative: { grid: true, domain: false },
    legend: {
      orient: "top",
      direction: "horizontal",
      padding: 0,
      columnPadding: 16,
      columns: compact ? 2 : 0,
      symbolType: "square",
      symbolSize: 100,
      labelColor: theme.ink2,
      labelFont: theme.font,
      labelFontSize: 12,
      titleColor: theme.ink2,
    },
    range: {
      category: palette === "ordered" ? theme.ramp : theme.categorical,
      ordinal: theme.ramp,
    },
    mark: { color: blue },
    // Vega-Lite has no maximum thickness for a bar: half of the band, which is
    // 24 px or less for the example, and never overlaps a neighbour.
    bar: { cornerRadiusEnd: 4, stroke: theme.surface, strokeWidth: 2, discreteBandSize: { band: 0.5 } },
    line: { strokeWidth: 2, strokeJoin: "round", strokeCap: "round" },
    area: { fillOpacity: 0.1, line: { strokeWidth: 2, strokeJoin: "round" } },
    point: { size: 64, filled: true, stroke: theme.surface, strokeWidth: 2 },
    circle: { size: 64, opacity: 0.7, stroke: theme.surface, strokeWidth: 2 },
    geoshape: { fill: theme.grid, stroke: null },
    text: { color: theme.ink2, font: theme.font, fontSize: 12 },
  };
}

/** The names a specification uses as geographic positions. */
function positionFields(node, found = new Set()) {
  if (Array.isArray(node)) {
    node.forEach((item) => positionFields(item, found));
  } else if (node !== null && typeof node === "object") {
    for (const [key, value] of Object.entries(node)) {
      if (/^(longitude|latitude)2?$/.test(key) && typeof value?.field === "string") found.add(value.field);
      positionFields(value, found);
    }
  }
  return found;
}

// --- the tween: what Vega lacks ---------------------------------------------
// A row is matched to the row of the old frame that has the same text, boolean
// and null values (its identity); the n-th duplicate matches the n-th. A matched
// row moves its numbers from the old values to the new ones. A row with no
// match grows from zero (or shrinks to zero, and is then dropped), except for
// geographic positions, which hold still. Easing is cubic-out.

const identity = (row) => JSON.stringify(Object.values(row).filter((v) => typeof v !== "number"));
const easeOut = (t) => 1 - (1 - t) ** 3;

function plan(from, to) {
  const waiting = new Map();
  for (const row of from) {
    const key = identity(row);
    if (!waiting.has(key)) waiting.set(key, []);
    waiting.get(key).push(row);
  }
  const pairs = to.map((row) => [waiting.get(identity(row))?.shift() ?? null, row]);
  const leaving = [...waiting.values()].flat();
  return { pairs, leaving };
}

function mix(old, now, t, still) {
  const row = { ...(now ?? old) };
  for (const [name, value] of Object.entries(row)) {
    if (typeof value !== "number") continue;
    if ((old === null || now === null) && still.has(name)) continue;
    const start = old === null ? 0 : typeof old[name] === "number" ? old[name] : value;
    const end = now === null ? 0 : value;
    row[name] = start + (end - start) * t;
  }
  return row;
}

function frame({ pairs, leaving }, t, still) {
  const rows = pairs.map(([old, now]) => mix(old, now, t, still));
  if (t < 1) for (const old of leaving) rows.push(mix(old, null, t, still));
  return rows;
}

// --- the tooltip -------------------------------------------------------------

function tipBox() {
  const box = document.createElement("div");
  box.className = "tip";
  box.hidden = true;
  document.body.append(box);
  return {
    show(entries, x, y) {
      box.replaceChildren(tooltipContent(entries));
      box.hidden = false;
      const { width, height } = box.getBoundingClientRect();
      const left = x + 14 + width > innerWidth ? x - 14 - width : x + 14;
      const top = y + 14 + height > innerHeight ? y - 14 - height : y + 14;
      box.style.left = `${Math.max(4, left)}px`;
      box.style.top = `${Math.max(4, top)}px`;
    },
    hide() {
      box.hidden = true;
    },
    remove() {
      box.remove();
    },
  };
}

const entriesOf = (value) =>
  Object.entries(value).map(([label, v]) => ({ value: formatCell(v), label }));

/** Every item of the scene that carries a tooltip, in drawing order. */
function tooltipItems(node, found = []) {
  for (const item of node.items ?? []) {
    if (item.tooltip !== undefined) found.push(item);
    if (item.items !== undefined) tooltipItems(item, found);
  }
  return found;
}

/**
 * @param {HTMLElement} host
 * @param {{spec: object, palette: string}} def
 * @param {{theme: object, motion: boolean}} context
 */
export async function create(host, def, { theme, motion }) {
  const spec = structuredClone(def.spec);
  spec.data = { name: "rows" };
  spec.datasets = { rows: [] };
  if (usesWorld(spec)) spec.datasets.world = (await loadWorld()).features;
  spec.width = "container";
  spec.height = "container";
  spec.autosize = { type: "fit", contains: "padding" };
  const compact = host.clientWidth < COMPACT;
  spec.config = { ...houseConfig(theme, def.palette, compact), ...spec.config };
  // Room on the right for the value label of the longest bar.
  spec.padding = { left: 0, top: 4, right: 16, bottom: 0, ...spec.padding };

  const compiled = vegaLite.compile(spec).spec;
  const runtime = vega.parse(compiled, null, { ast: true });
  const tip = tipBox();
  const view = new vega.View(runtime, { expr: expressionInterpreter, renderer: "canvas", container: host });
  view.tooltip((handler, event, item, value) => {
    if (value === null || value === undefined || typeof value !== "object") tip.hide();
    else tip.show(entriesOf(value), event.clientX, event.clientY);
  });
  await view.runAsync();

  const still = positionFields(def.spec);
  const ring = document.createElement("div");
  ring.className = "focus-ring";
  ring.hidden = true;
  host.parentElement.append(ring);

  let shown = [];
  let run = 0;
  let focused = -1;

  const apply = (rows) => {
    shown = rows;
    view.change("rows", vega.changeset().remove(vega.truthy).insert(rows.map((row) => ({ ...row }))));
    return view.runAsync();
  };

  /** Starts the change; `first` is the first frame drawn, `done` the last. */
  function tween(rows) {
    const mine = ++run;
    const steps = plan(shown, rows);
    if (!motion) {
      const only = apply(rows);
      return { first: only, done: only };
    }
    const start = performance.now();
    const first = apply(frame(steps, 0, still));
    const done = first.then(
      () =>
        new Promise((resolve) => {
          const tick = async (now) => {
            if (mine !== run) return resolve();
            const t = Math.min(1, (now - start) / DURATION);
            await (t < 1 ? apply(frame(steps, easeOut(t), still)) : apply(rows));
            if (t < 1 && mine === run) requestAnimationFrame(tick);
            else resolve();
          };
          requestAnimationFrame(tick);
        }),
    );
    return { first, done };
  }

  function place(item) {
    const canvas = host.querySelector("canvas");
    const box = canvas.getBoundingClientRect();
    const [ox, oy] = view.origin();
    const b = item.bounds;
    return { box, left: ox + b.x1, top: oy + b.y1, width: b.width(), height: b.height() };
  }

  function stepTo(index) {
    const items = tooltipItems(view.scenegraph().root);
    if (items.length === 0) return;
    focused = Math.min(items.length - 1, Math.max(0, index));
    const { box, left, top, width, height } = place(items[focused]);
    const parent = host.parentElement.getBoundingClientRect();
    ring.hidden = false;
    ring.style.left = `${box.left - parent.left + left - 2}px`;
    ring.style.top = `${box.top - parent.top + top - 2}px`;
    ring.style.width = `${width + 4}px`;
    ring.style.height = `${height + 4}px`;
    tip.show(entriesOf(items[focused].tooltip), box.left + left + width, box.top + top);
  }

  return {
    /** Draws `rows`, animating from what is shown. Resolves when the first frame is drawn. */
    async update(rows) {
      focused = -1;
      ring.hidden = true;
      const { first, done } = tween(rows);
      // A failure in a later frame has no caller to return to: let it reach
      // the console as an uncaught error rather than vanish.
      done.catch((error) => setTimeout(() => { throw error; }));
      await first;
    },
    /** Fits the view to its box. Resolves to true when the chart must be built again. */
    async resize() {
      if (host.clientWidth < COMPACT !== compact) return true;
      await view.resize().runAsync();
      return false;
    },
    step(delta) {
      stepTo(focused < 0 ? (delta > 0 ? 0 : Number.MAX_SAFE_INTEGER) : focused + delta);
    },
    stepTo,
    clear() {
      focused = -1;
      ring.hidden = true;
      tip.hide();
    },
    dispose() {
      run += 1;
      view.finalize();
      tip.remove();
      ring.remove();
      host.replaceChildren();
    },
  };
}
