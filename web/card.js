// The cards: a stat tile and a chart card. Each shows what its query last
// returned, in one of four states: loading, ready, empty or error. An error
// shows the message and the id the server logged; an empty result says so.
// A card never shows a blank chart that could be read as zero.

import { MAP_MARGIN, create } from "./chart.js";
import { formatCell, formatNumber } from "./format.js";
import { readTheme, reducedMotion } from "./theme.js";
import { aspect, loadWorld, usesWorld } from "./world.js";

const TABLE_ROWS = 500;

/** A failed query: the server's message and the id it logged under. */
export class QueryError extends Error {
  constructor(message, id) {
    super(message);
    this.id = id;
  }
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function describeError(error) {
  const box = element("div", "error");
  box.append(element("p", "error-message", error.message));
  if (error.id) box.append(element("p", "error-id", `Error id: ${error.id}`));
  return box;
}

/** A card that waits for a query. `load` takes the promise of its rows. */
class Card {
  constructor(className, retry) {
    this.root = element("section", `card ${className}`);
    this.root.dataset.state = "loading";
    this.retry = retry;
    this.token = 0;
    this.hasContent = false;
  }

  /** Waits for the query, then shows what came back. */
  async load(pending) {
    const mine = ++this.token;
    this.root.dataset.state = this.hasContent ? "refreshing" : "loading";
    let result;
    try {
      result = await pending;
    } catch (error) {
      if (mine === this.token) await this.#settle(() => this.fail(error));
      return;
    }
    if (mine !== this.token) return;
    await this.#settle(() => (result.rows.length === 0 ? this.empty() : this.show(result)));
  }

  async #settle(show) {
    try {
      await show();
    } catch (error) {
      this.fail(error);
    }
  }
}

const easeOut = (t) => 1 - (1 - t) ** 3;
const COUNT_MS = 300;

export class StatCard extends Card {
  constructor(stat, retry) {
    super(stat.hero ? "stat hero" : "stat", retry);
    this.stat = stat;
    this.query = stat.query;
    this.display = null; // the number on the screen now, while it counts
    this.counting = 0;
    this.label = element("div", "stat-label", stat.label);
    this.value = element("div", "stat-value", "…");
    const body = element("div", "stat-body");
    body.append(this.label, this.value);
    this.root.append(body);
    if (stat.trend) {
      this.root.classList.add("has-trend");
      this.spark = new Sparkline(stat, retry);
      this.root.append(this.spark.root);
    }
  }

  /** The queries this tile reads: its number's, and its trend's. */
  get queries() {
    return new Set([this.stat.query, this.stat.trend?.query].filter(Boolean));
  }

  load(pending, name) {
    const jobs = [];
    if (name === this.stat.query) jobs.push(super.load(pending));
    if (name === this.stat.trend?.query) jobs.push(this.spark.load(pending));
    return Promise.all(jobs);
  }

  /** The number counts to its new value, from the one on the screen. */
  async #count(to) {
    const from = this.display ?? 0;
    const mine = ++this.counting;
    const write = (value) => {
      this.display = value;
      this.value.textContent = formatNumber(value, this.stat.format);
    };
    if (reducedMotion() || from === to) return write(to);
    const start = performance.now();
    await new Promise((resolve) => {
      const tick = (now) => {
        if (mine !== this.counting) return resolve();
        const t = Math.min(1, (now - start) / COUNT_MS);
        write(t < 1 ? from + (to - from) * easeOut(t) : to);
        if (t < 1) requestAnimationFrame(tick);
        else resolve();
      };
      requestAnimationFrame(tick);
    });
  }

  async show({ rows }) {
    if (rows.length !== 1) throw new Error(`the query returned ${rows.length} rows; a stat needs exactly one`);
    const raw = rows[0][this.stat.value];
    if (raw === null) return this.empty();
    this.value.className = "stat-value";
    await this.#count(raw);
    this.hasContent = true;
    this.root.dataset.state = "ready";
  }

  empty() {
    this.counting += 1;
    this.display = null;
    this.value.className = "stat-value stat-none";
    this.value.textContent = "No data";
    this.hasContent = true;
    this.root.dataset.state = "empty";
  }

  fail(error) {
    this.counting += 1;
    this.display = null;
    this.value.className = "stat-value stat-none";
    this.value.replaceChildren(describeError(error instanceof QueryError ? error : new QueryError(error.message, null)));
    this.hasContent = true;
    this.root.dataset.state = "error";
  }
}

/**
 * The small line beside a stat: the muted colour, no axes, the last point marked
 * with its value. It has its own state, so that a trend that
 * fails or is empty says so in its place and the number is left alone.
 */
class Sparkline {
  constructor(stat, retry) {
    this.stat = stat;
    this.retry = retry;
    this.token = 0;
    this.chart = null;
    this.rows = null;
    this.root = element("div", "spark");
    this.root.dataset.state = "loading";
    this.root.setAttribute("role", "img");
    this.root.setAttribute("aria-label", `${stat.label}: trend`);
    this.canvas = element("div", "spark-canvas");
    this.note = element("div", "spark-note");
    this.note.hidden = true;
    this.root.append(this.canvas, this.note);
    new ResizeObserver(() => this.chart?.resize()).observe(this.canvas);
    document.addEventListener("themechange", () => this.#draw(false));
  }

  async load(pending) {
    const mine = ++this.token;
    this.root.dataset.state = "loading";
    let result;
    try {
      result = await pending;
    } catch (error) {
      if (mine === this.token) this.#say("error", error);
      return;
    }
    if (mine !== this.token) return;
    if (result.rows.length === 0) return this.#say("empty");
    this.rows = result.rows;
    this.note.hidden = true;
    this.canvas.hidden = false;
    try {
      this.#draw(true);
      this.root.dataset.state = "ready";
    } catch (error) {
      this.#say("error", error);
    }
  }

  #say(state, error) {
    this.chart?.dispose();
    this.chart = null;
    this.canvas.hidden = true;
    this.note.hidden = false;
    this.root.dataset.state = state;
    if (state === "empty") {
      this.note.textContent = "No data for the trend";
      return;
    }
    const shown = error instanceof QueryError ? error : new QueryError(error.message, null);
    this.note.replaceChildren(describeError(shown));
  }

  #draw(animate) {
    if (this.rows === null || this.canvas.hidden) return;
    const { x, value } = this.stat.trend;
    const theme = readTheme();
    const motion = !reducedMotion();
    const points = this.rows.map((row) => [row[x], row[value]]);
    const last = points[points.length - 1];
    const option = {
      backgroundColor: "transparent",
      animation: motion && animate,
      animationDuration: 600,
      animationDurationUpdate: 300,
      animationEasing: "cubicOut",
      animationEasingUpdate: "cubicOut",
      textStyle: { fontFamily: theme.font },
      grid: { left: 4, right: 20, top: 22, bottom: 6 },
      xAxis: { type: "category", show: false, boundaryGap: false },
      yAxis: { type: "value", show: false, min: 0 },
      series: [
        {
          type: "line",
          silent: true,
          data: points,
          showSymbol: false,
          lineStyle: { color: theme.muted, width: 2, join: "round", cap: "round" },
          markPoint: {
            animation: false,
            symbol: "circle",
            symbolSize: 8,
            itemStyle: { color: theme.categorical[0], borderColor: theme.surface, borderWidth: 2 },
            label: { show: true, position: "top", color: theme.ink2, fontSize: 12, fontFamily: theme.font, formatter: () => formatNumber(last[1]) },
            data: [{ coord: [last[0], last[1]] }],
          },
        },
      ],
    };
    if (this.chart !== null && !animate) {
      this.chart.dispose();
      this.chart = null;
    }
    this.chart ??= echarts.init(this.canvas, null, { renderer: "canvas" });
    this.chart.setOption(option);
  }
}

export class ChartCard extends Card {
  constructor(chart, retry) {
    super("chart", retry);
    this.chart = chart;
    this.query = chart.query;
    this.drawing = null;
    this.rows = null;
    this.columns = null;

    const title = element("h2", "chart-title", chart.title);
    this.toggle = element("button", "toggle", "Table");
    this.toggle.type = "button";
    this.toggle.setAttribute("aria-pressed", "false");
    this.toggle.addEventListener("click", () => this.#showTable(this.toggle.getAttribute("aria-pressed") !== "true"));
    const header = element("header", "chart-head");
    header.append(title, this.toggle);

    this.plot = element("div", "plot");
    this.host = element("div", "host");
    this.host.tabIndex = 0;
    this.host.setAttribute("role", "img");
    this.host.setAttribute("aria-label", `${chart.title}. Use the left and right arrow keys to read the values.`);
    this.message = element("div", "state");
    this.message.setAttribute("role", "status");
    this.plot.append(this.host, this.message);
    this.tableBox = element("div", "table-wrap");
    this.tableBox.hidden = true;
    this.root.append(header, this.plot, this.tableBox);

    this.host.addEventListener("keydown", (event) => this.#key(event));
    this.host.addEventListener("blur", () => this.drawing?.clear());
    // A chart that draws the world map is as tall as the outline needs for the
    // card's width: no empty band above or below it, at any width.
    this.mapShape = null;
    if (usesWorld(chart.spec)) {
      loadWorld().then((world) => {
        this.mapShape = aspect(world);
        this.#fit();
      });
    }
    new ResizeObserver(async () => {
      this.#fit();
      if (this.drawing === null || this.plot.hidden) return;
      if (await this.drawing.resize()) await this.#rebuild();
    }).observe(this.plot);
    document.addEventListener("themechange", () => this.#rebuild());
    this.#say("Loading…");
  }

  #fit() {
    const width = this.plot.clientWidth;
    if (this.mapShape === null || width === 0) return;
    this.plot.style.height = `${Math.round((width - 2 * MAP_MARGIN) / this.mapShape) + 2 * MAP_MARGIN}px`;
  }

  #say(content) {
    this.message.replaceChildren(content);
    this.message.hidden = false;
  }

  #key(event) {
    if (this.drawing === null) return;
    const handled = { ArrowRight: () => this.drawing.step(1), ArrowLeft: () => this.drawing.step(-1), Home: () => this.drawing.stepTo(0), End: () => this.drawing.stepTo(Infinity), Escape: () => this.drawing.clear() };
    if (handled[event.key] === undefined) return;
    event.preventDefault();
    handled[event.key]();
  }

  async #draw(rows, animate) {
    if (this.drawing === null) {
      this.drawing = await create(this.host, this.chart, {
        theme: readTheme(),
        motion: !reducedMotion(),
      });
    }
    await this.drawing.update(rows, { animate });
  }

  /** Builds the chart again from the rows already held: after a new theme or a change of layout. */
  async #rebuild() {
    if (this.drawing === null || this.rows === null) return;
    this.drawing.dispose();
    this.drawing = null;
    try {
      await this.#draw(this.rows, false);
    } catch (error) {
      this.fail(error);
    }
  }

  #showTable(on) {
    this.toggle.setAttribute("aria-pressed", String(on));
    this.tableBox.hidden = !on;
    this.plot.hidden = on;
    if (on) this.#renderTable();
    else if (this.drawing !== null) Promise.resolve(this.drawing.resize()).then((again) => again && this.#rebuild());
  }

  #renderTable() {
    if (this.rows === null) {
      this.tableBox.replaceChildren(element("p", "table-note", "No rows to show."));
      return;
    }
    const table = element("table");
    const head = element("tr");
    for (const column of this.columns) head.append(element("th", "", column));
    const thead = element("thead");
    thead.append(head);
    table.append(thead);
    const body = element("tbody");
    for (const row of this.rows.slice(0, TABLE_ROWS)) {
      const tr = element("tr");
      for (const column of this.columns) tr.append(element("td", "", formatCell(row[column])));
      body.append(tr);
    }
    table.append(body);
    const parts = [table];
    if (this.rows.length > TABLE_ROWS) {
      parts.push(element("p", "table-note", `Showing the first ${TABLE_ROWS} of ${this.rows.length} rows.`));
    }
    this.tableBox.replaceChildren(...parts);
  }

  async show({ columns, rows }) {
    this.rows = rows;
    this.columns = columns;
    this.host.hidden = false;
    this.message.hidden = true;
    await this.#draw(rows, true);
    if (!this.tableBox.hidden) this.#renderTable();
    this.hasContent = true;
    this.root.dataset.state = "ready";
    if (performance.getEntriesByName("first-chart").length === 0) performance.mark("first-chart");
  }

  empty() {
    this.rows = [];
    this.host.hidden = true;
    this.#say("Nothing to show: no rows match the current filters.");
    if (!this.tableBox.hidden) this.#renderTable();
    this.hasContent = true;
    this.root.dataset.state = "empty";
  }

  fail(error) {
    const shown = error instanceof QueryError ? error : new QueryError(error.message, null);
    this.host.hidden = true;
    const box = describeError(shown);
    const again = element("button", "retry", "Try again");
    again.type = "button";
    again.addEventListener("click", () => this.retry(this.query));
    box.append(again);
    this.#say(box);
    this.hasContent = true;
    this.root.dataset.state = "error";
  }
}
