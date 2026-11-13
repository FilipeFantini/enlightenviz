// The cards: a stat tile and a chart card. Each shows what its query last
// returned, in one of four states: loading, ready, empty or error. An error
// shows the message and the id the server logged; an empty result says so.
// A card never shows a blank chart that could be read as zero.

import { formatCell, formatNumber } from "./format.js";
import { readTheme, reducedMotion } from "./theme.js";

// Loaded on demand: a board that has no Vega-Lite chart never loads the Vega
// adapter, which needs the Vega library the page does not include for it.
const adapters = {
  vega_lite: () => import("./chart-vega.js"),
  echarts: () => import("./chart-echarts.js"),
};
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

export class StatCard extends Card {
  constructor(stat, retry) {
    super(stat.hero ? "stat hero" : "stat", retry);
    this.stat = stat;
    this.query = stat.query;
    this.label = element("div", "stat-label", stat.label);
    this.value = element("div", "stat-value", "…");
    this.root.append(this.label, this.value);
  }

  show({ rows }) {
    if (rows.length !== 1) throw new Error(`the query returned ${rows.length} rows; a stat needs exactly one`);
    const raw = rows[0][this.stat.value];
    if (raw === null) return this.empty();
    this.value.className = "stat-value";
    this.value.textContent = formatNumber(raw, this.stat.format);
    this.hasContent = true;
    this.root.dataset.state = "ready";
  }

  empty() {
    this.value.className = "stat-value stat-none";
    this.value.textContent = "No data";
    this.hasContent = true;
    this.root.dataset.state = "empty";
  }

  fail(error) {
    this.value.className = "stat-value stat-none";
    this.value.replaceChildren(describeError(error instanceof QueryError ? error : new QueryError(error.message, null)));
    this.hasContent = true;
    this.root.dataset.state = "error";
  }
}

export class ChartCard extends Card {
  constructor(chart, retry) {
    super("chart", retry);
    this.chart = chart;
    this.query = chart.query;
    this.adapter = null;
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
    this.host.addEventListener("blur", () => this.adapter?.clear());
    new ResizeObserver(async () => {
      if (this.adapter === null || this.plot.hidden) return;
      if (await this.adapter.resize()) await this.#rebuild();
    }).observe(this.plot);
    document.addEventListener("themechange", () => this.#rebuild());
    this.#say("Loading…");
  }

  #say(content) {
    this.message.replaceChildren(content);
    this.message.hidden = false;
  }

  #key(event) {
    if (this.adapter === null) return;
    const handled = { ArrowRight: () => this.adapter.step(1), ArrowLeft: () => this.adapter.step(-1), Home: () => this.adapter.stepTo(0), End: () => this.adapter.stepTo(Infinity), Escape: () => this.adapter.clear() };
    if (handled[event.key] === undefined) return;
    event.preventDefault();
    handled[event.key]();
  }

  async #draw(rows, animate) {
    if (this.adapter === null) {
      this.adapter = await (await adapters[this.chart.engine]()).create(this.host, this.chart, {
        theme: readTheme(),
        motion: !reducedMotion(),
      });
    }
    await this.adapter.update(rows, { animate });
  }

  /** Builds the chart again from the rows already held: after a new theme or a change of layout. */
  async #rebuild() {
    if (this.adapter === null || this.rows === null) return;
    this.adapter.dispose();
    this.adapter = null;
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
    else if (this.adapter !== null) Promise.resolve(this.adapter.resize()).then((again) => again && this.#rebuild());
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
