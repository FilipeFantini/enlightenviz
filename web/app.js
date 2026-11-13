// The board page: reads the board's definition, builds the filter row, the
// stat tiles and the chart cards, and asks each query for its rows. A change
// of filter asks again only the queries that declare that filter's parameter.

import { ChartCard, QueryError, StatCard } from "./card.js";
import { buildControls, stateFromUrl, writeUrl } from "./controls.js";
import { scheme, toggleScheme } from "./theme.js";

const app = document.getElementById("app");

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** GET a JSON answer; an error answer becomes a QueryError with its id. */
async function getJson(url, signal) {
  let response;
  try {
    response = await fetch(url, { signal });
  } catch (error) {
    if (error.name === "AbortError") throw error;
    throw new QueryError("The server could not be reached.", null);
  }
  let body;
  try {
    body = await response.json();
  } catch {
    throw new QueryError(`The server answered ${response.status} with something that is not JSON.`, null);
  }
  if (!response.ok) {
    const { message, id } = body.error ?? {};
    throw new QueryError(message ?? `The server answered ${response.status}.`, id ?? null);
  }
  return body;
}

function showFailure(error) {
  const box = element("div", "card failure");
  box.append(element("h1", "board-title", "This board cannot be shown"));
  box.append(element("p", "error-message", error.message));
  if (error.id) box.append(element("p", "error-id", `Error id: ${error.id}`));
  app.replaceChildren(box);
}

function themeButton() {
  const button = element("button", "toggle theme-toggle");
  button.type = "button";
  const label = () => {
    button.textContent = scheme() === "dark" ? "Light colours" : "Dark colours";
  };
  label();
  button.addEventListener("click", () => {
    toggleScheme();
    label();
  });
  document.addEventListener("themechange", label);
  return button;
}

/**
 * Each filter's options as `{value, label}` with text values (null if they
 * could not be loaded), and how many rows of an options query had no value.
 * A NULL is never offered: sent as a parameter it would be the text "null".
 */
async function loadOptions(definition) {
  const options = {};
  const skipped = {};
  await Promise.all(
    definition.filters.map(async (filter) => {
      if (filter.options) {
        options[filter.param] = filter.options.map((o) => ({ value: String(o.value), label: o.label }));
      } else if (filter.options_from) {
        const { query, value, label } = filter.options_from;
        try {
          const { rows } = await getJson(`/api/boards/${definition.id}/queries/${query}`);
          const offered = rows.filter((row) => row[value] !== null);
          skipped[filter.param] = rows.length - offered.length;
          options[filter.param] = offered.map((row) => ({ value: String(row[value]), label: String(row[label] ?? row[value]) }));
        } catch (error) {
          console.error(`The options of ${filter.label} could not be loaded: ${error.message}`);
          options[filter.param] = null;
        }
      }
    }),
  );
  return { options, skipped };
}

async function main() {
  const id = decodeURIComponent(location.pathname.split("/").filter(Boolean).pop() ?? "");
  const definition = await getJson(`/api/boards/${encodeURIComponent(id)}`);

  const { options, skipped } = await loadOptions(definition);
  const { values, notices } = stateFromUrl(definition.filters, location.search, options);

  const top = element("header", "top");
  const heading = element("div");
  heading.append(element("h1", "board-title", definition.title));
  if (definition.text) heading.append(element("p", "board-text", definition.text));
  top.append(heading, themeButton());

  const filterRow = element("div", "filters");
  filterRow.setAttribute("role", "group");
  filterRow.setAttribute("aria-label", "Filters");
  const notice = element("div", "notices");
  notice.setAttribute("role", "status");
  for (const text of notices) notice.append(element("p", "notice", text));

  const pending = new Map(); // query name -> AbortController of its request in flight
  const cards = new Map(); // query name -> its cards

  function refresh(names) {
    for (const name of names) {
      pending.get(name)?.abort();
      const controller = new AbortController();
      pending.set(name, controller);
      const params = new URLSearchParams();
      for (const filter of definition.filters) {
        if (!(filter.param in definition.queries[name].params)) continue;
        for (const item of [].concat(values[filter.param])) params.append(filter.param, item);
      }
      const request = getJson(`/api/boards/${definition.id}/queries/${name}?${params}`, controller.signal);
      for (const card of cards.get(name)) card.load(request, name);
    }
  }
  const retry = (name) => refresh([name]);

  const stats = element("div", "stats");
  const charts = element("div", "charts");
  // The tiles beside the hero figure lay out in one clean row at every width
  // (or one column on a phone), never with an orphan: the columns are counted.
  const tiles = definition.stats.filter((stat) => !stat.hero).length;
  const hero = definition.stats.some((stat) => stat.hero);
  stats.style.setProperty("--wide", String(hero ? tiles + 2 : tiles));
  stats.style.setProperty("--mid", String(Math.max(1, Math.min(tiles || 1, 4))));
  for (const stat of definition.stats) {
    const card = new StatCard(stat, retry);
    stats.append(card.root);
    for (const name of card.queries) cards.set(name, [...(cards.get(name) ?? []), card]);
  }
  for (const chart of definition.charts) {
    const card = new ChartCard(chart, retry);
    charts.append(card.root);
    cards.set(chart.query, [...(cards.get(chart.query) ?? []), card]);
  }

  buildControls(filterRow, definition.filters, options, skipped, values, (param) => {
    writeUrl(definition.filters, values);
    notice.replaceChildren();
    refresh([...cards.keys()].filter((name) => param in definition.queries[name].params));
  });

  const parts = [top];
  if (definition.filters.length > 0) parts.push(filterRow);
  parts.push(notice);
  if (definition.stats.length > 0) parts.push(stats);
  parts.push(charts);
  app.replaceChildren(...parts);
  refresh([...cards.keys()]);
}

main().catch((error) => {
  console.error(error);
  showFailure(error instanceof QueryError ? error : new QueryError(error.message, null));
});
