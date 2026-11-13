// The browser smoke test, and the closed-network test of design section 8.
//
// Run by run.sh on a Docker network with no route to the internet. For each
// board: the page loads, every chart draws (a canvas with pixels in it), the
// console has no error and no CSP violation, no request goes to another host,
// a filter changes the numbers, the empty and error states are explicit, the
// table view lists the rows and the colour toggle works. The "hostile" board
// is checked differently: its data is markup, and must be shown as text.
// Prints one JSON object of measurements at the end; exits 1 if a check failed.

import { ALL_SETTLED, startBrowser } from "./cdp.mjs";

const [base, ...boards] = process.argv.slice(2);
const host = new URL(base).host;
const failures = [];

function check(board, ok, what) {
  console.log(`${ok ? "ok  " : "FAIL"} ${board}: ${what}`);
  if (!ok) failures.push(`${board}: ${what}`);
}

// The network really is closed: nothing outside answers.
async function closedNetwork() {
  for (const url of ["https://example.com/", "http://1.1.1.1/"]) {
    let reached = false;
    try {
      await fetch(url, { signal: AbortSignal.timeout(5000) });
      reached = true;
    } catch {
      // expected
    }
    check("network", !reached, `${url} is unreachable from the test container`);
  }
}

const hasPixels = `[...document.querySelectorAll(".card.chart .host canvas")].map((c) => {
  const data = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
  for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) return true;
  return false;
})`;

async function smoke(browser, board) {
  const page = await browser.newPage({ width: 1440, height: 900, scheme: "light" });
  const url = `${base}/boards/${board}`;
  // The colour choice is kept in the browser's storage, which both boards share:
  // start from none.
  await page.goto(`${base}/`);
  await page.waitFor(`document.readyState === "complete"`, "the index page");
  await page.evaluate("localStorage.clear()");
  await page.goto(url);
  await page.waitFor(ALL_SETTLED, "every card to settle");
  await new Promise((resolve) => setTimeout(resolve, 800)); // let the animation end

  const states = await page.evaluate(`[...document.querySelectorAll(".card.chart, .card.stat")].map((c) => c.dataset.state)`);
  check(board, states.length === 7 && states.every((s) => s === "ready"), `4 stats and 3 charts are ready (${states.join(",")})`);
  const drawn = await page.evaluate(hasPixels);
  check(board, drawn.length === 3 && drawn.every(Boolean), `each of the 3 charts drew a canvas with content (${drawn.join(",")})`);
  const spark = await page.evaluate(`(() => {
    const spark = document.querySelector(".stat.hero .spark");
    const canvas = spark?.querySelector("canvas");
    if (!canvas) return "no canvas, state " + spark?.dataset.state;
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) return "drawn";
    return "empty canvas";
  })()`);
  check(board, spark === "drawn", `the hero figure's trend is drawn (${spark})`);
  const firstChart = await page.evaluate(`performance.getEntriesByName("first-chart")[0]?.startTime`);
  check(board, typeof firstChart === "number", `time to the first chart: ${Math.round(firstChart)} ms`);
  const hero = await page.evaluate(`document.querySelector(".stat.hero .stat-value").textContent`);
  check(board, /^\d/.test(hero), `the hero figure is a number (${hero})`);

  // A filter changes the numbers, and the link reproduces the view.
  await page.evaluate(`(() => { const s = document.querySelector("select"); s.value = "5"; s.dispatchEvent(new Event("change")); })()`);
  await page.waitFor(`document.querySelector(".stat.hero .stat-value").textContent !== ${JSON.stringify(hero)}`, "the hero figure to change");
  await page.waitFor(ALL_SETTLED, "cards to settle after the filter");
  await new Promise((resolve) => setTimeout(resolve, 800)); // let the animation end
  check(board, (await page.evaluate("location.search")) === "?min_magnitude=5", "the filter is in the URL");
  const filtered = await page.evaluate(`document.querySelector(".stat.hero .stat-value").textContent`);
  check(board, Number(filtered) < Number(hero), `the filter lowered the hero figure (${hero} to ${filtered})`);

  // Keyboard: a chart takes focus, and an arrow key shows a tooltip with the
  // value first (the ranked bar chart's first bar is Aegean, 60 events or more).
  await page.evaluate(`document.querySelectorAll(".card.chart .host")[1].focus()`);
  await page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
  await new Promise((resolve) => setTimeout(resolve, 500));
  const tip = await page.evaluate(`[...document.querySelectorAll(".tip:not([hidden]), body > div:not(#app)")]
    .filter((n) => getComputedStyle(n).display !== "none" && n.textContent.trim() !== "")
    .map((n) => n.textContent).join(" | ")`);
  check(board, /^\d+\s*Events/.test(tip), `an arrow key on a focused chart shows a tooltip, value first (${tip})`);

  // The table view lists the rows.
  await page.evaluate(`document.querySelector(".chart .toggle").click()`);
  const tableRows = await page.evaluate(`document.querySelectorAll(".chart .table-wrap:not([hidden]) tbody tr").length`);
  check(board, tableRows > 0, `the table view lists ${tableRows} rows`);
  await page.evaluate(`document.querySelector(".chart .toggle").click()`);

  // The colour toggle wins, and the charts draw again under it.
  await page.evaluate(`document.querySelector(".theme-toggle").click()`);
  await page.waitFor(`document.documentElement.dataset.theme === "dark"`, "the dark theme");
  await page.waitFor(ALL_SETTLED, "charts to settle after the toggle");
  const darkDrawn = await page.evaluate(hasPixels);
  check(board, darkDrawn.length === 3 && darkDrawn.every(Boolean), "the charts draw again after the colour toggle");

  // An empty result is said, not drawn as zero.
  await page.goto(`${url}?from=2030-01-01`);
  await page.waitFor(ALL_SETTLED, "cards to settle for an empty result");
  const empty = await page.evaluate(`[...document.querySelectorAll(".card.chart")].map((c) => c.dataset.state + ":" + c.querySelector(".state").textContent)`);
  check(board, empty.every((s) => s.startsWith("empty:Nothing to show")), `an empty result says so (${empty.join(" | ")})`);

  // A failing query shows its message and its id.
  await page.goto(`${url}?from=2024-02-31`);
  await page.waitFor(ALL_SETTLED, "cards to settle for an error");
  const errors = await page.evaluate(`[...document.querySelectorAll(".card.chart")].map((c) => c.dataset.state + ":" + c.querySelector(".state").textContent)`);
  check(board, errors.every((s) => s.startsWith("error:") && /Error id: [0-9a-f]+-[0-9a-f]+/.test(s)), `a failed query shows its message and id (${errors[0]})`);

  // On a phone nothing overflows, and the tiles are rows of the same width.
  await page.goto(url);
  await page.send("Emulation.setDeviceMetricsOverride", { width: 375, height: 900, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(ALL_SETTLED, "cards to settle at 375 px");
  await new Promise((resolve) => setTimeout(resolve, 800));
  const narrow = await page.evaluate(`({
    overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    widths: [...document.querySelectorAll(".stat")].map((n) => Math.round(n.getBoundingClientRect().width)),
  })`);
  check(board, narrow.overflow <= 0, `at 375 px nothing overflows (${narrow.overflow} px)`);
  check(board, new Set(narrow.widths).size === 1, `at 375 px the stat tiles all have one width (${narrow.widths})`);

  const other = page.requests.filter((u) => new URL(u).host !== host && !u.startsWith("data:"));
  check(board, other.length === 0, `every request went to ${host} (${page.requests.length} requests${other.length ? `, others: ${other}` : ""})`);
  // The error check above provokes 400 responses, which the browser logs as
  // errors of their own; only those are expected.
  const unexpected = page.errors.filter((e) => !/status of 400/.test(e));
  check(board, unexpected.length === 0, `no console error or CSP violation${unexpected.length ? `: ${unexpected.join(" || ")}` : ""}`);
  if (page.warnings.length) console.log(`note ${board}: ${page.warnings.length} console warning(s): ${page.warnings.slice(0, 3).join(" || ")}`);
  page.close();

  // With reduced motion the number is at its new value at once.
  const calm = await browser.newPage({ width: 1440, height: 900, scheme: "light", reducedMotion: true });
  await calm.goto(url);
  await calm.waitFor(ALL_SETTLED, "cards to settle with reduced motion");
  await calm.evaluate(`(() => { const s = document.querySelector("select"); s.value = "5"; s.dispatchEvent(new Event("change")); })()`);
  await calm.waitFor(`document.querySelector(".stat.hero .stat-value").textContent !== "360"`, "the hero figure to change");
  const at = await calm.evaluate(`document.querySelector(".stat.hero .stat-value").textContent`);
  check(board, at === filtered, `with reduced motion the figure is at its new value at once (${at})`);
  calm.close();
  return { board, firstChart: Math.round(firstChart), requests: page.requests.length };
}

// The rows of the hostile board hold markup that would draw a picture, make
// text bold or run a script if the page treated it as HTML. It must appear
// as text everywhere, and the tooltip must hold only what the page built.
const elementsOfMarkup = `document.querySelectorAll("img, b, i, u, iframe, object, embed, script:not([src]), [onerror]").length`;
const hostileText = (text) => (value) => value.includes(text);

async function hostile(browser, board) {
  const page = await browser.newPage({ width: 1440, height: 900, scheme: "light" });
  const url = `${base}/boards/${board}`;
  await page.goto(url);
  await page.waitFor(ALL_SETTLED, "every card to settle");
  await new Promise((resolve) => setTimeout(resolve, 800));
  const noMarkup = async (where) => {
    const found = await page.evaluate(elementsOfMarkup);
    const ran = await page.evaluate(`window.pwned`);
    check(board, found === 0 && ran === undefined, `${where}: no element came from the data, and nothing ran (${found} found, pwned=${ran})`);
  };
  const states = await page.evaluate(`[...document.querySelectorAll(".card.chart, .card.stat")].map((c) => c.dataset.state + (c.dataset.state === "ready" ? "" : ":" + c.querySelector(".state, .error-message")?.textContent))`);
  check(board, states.length === 4 && states.every((s) => s === "ready"), `2 stats and 2 charts are ready (${states.join(",")})`);
  await noMarkup("after load");

  // Text a board author wrote and text the data holds are both shown literally.
  const titles = await page.evaluate(`[...document.querySelectorAll(".card .chart-title, .card .stat-label")].map((n) => n.textContent)`);
  check(board, titles.includes("<b>In place</b>") && titles.some(hostileText("<b>Total</b> by place")), `a title and a tile label with markup are shown as text (${titles.join(" | ")})`);

  // Filter options: each hostile label is an option, as text. The NULL one is
  // not offered, and the control says so.
  const place = await page.evaluate(`[...document.querySelectorAll("select option")].map((o) => o.value + "=>" + o.textContent)`);
  check(board, place.length === 3 && place.every((o) => !/^null=>|=>null$/.test(o)) && place.some(hostileText('<img src=x onerror="window.pwned = 1">')), `the select offers the 3 options with a value, as text, and no "null" (${place.join(" | ")})`);
  const boxes = await page.evaluate(`[...document.querySelectorAll(".multi input")].map((b) => b.value)`);
  check(board, boxes.length === 2 && !boxes.includes("null"), `the multi-select offers the 2 options with a value (${boxes.join(" | ")})`);
  const notes = await page.evaluate(`[...document.querySelectorAll(".filter-note")].filter((n) => !n.hidden && getComputedStyle(n).display !== "none").map((n) => n.textContent)`);
  const noValue = notes.filter((n) => /no value and is not shown/.test(n));
  check(board, noValue.length === 2, `both controls say that an option has no value and is not shown (${notes.join(" | ")})`);
  // The default place is not among the options: the control says which value is in use.
  check(board, notes.some((n) => n.includes("<i>no such place</i>") && /not among the options/.test(n)), "a value in use that is not among the options is named at its control");

  // A tooltip holds only what the page built: divs, strongs and spans, with the
  // text literally. Arrow keys walk the bars; the third one is the script row.
  const tips = [];
  // Each chart has a tooltip element of its own; the one showing is visible.
  const tipOf = () => `(() => {
    const showing = (n) => { const s = getComputedStyle(n.parentElement); return s.display !== "none" && s.visibility !== "hidden" && Number(s.opacity) > 0; };
    const lines = [...document.querySelectorAll(".tip-lines")].filter(showing).pop();
    if (!lines) return null;
    const tags = [...lines.parentElement.querySelectorAll("*")].map((n) => n.tagName);
    return { text: lines.textContent, tags: [...new Set(tags)] };
  })()`;
  await page.evaluate(`document.querySelectorAll(".card.chart .host")[0].focus()`);
  for (let i = 0; i < 3; i++) {
    await page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
    await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
    await new Promise((resolve) => setTimeout(resolve, 500));
    tips.push(await page.evaluate(tipOf()));
  }
  await page.evaluate(`document.querySelectorAll(".card.chart .host")[1].focus()`);
  await page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
  await new Promise((resolve) => setTimeout(resolve, 500));
  tips.push(await page.evaluate(tipOf()));
  const shown = tips.map((t) => t?.text ?? "(no tooltip)");
  check(board, tips.every((t) => t !== null), `each arrow key showed a tooltip (${shown.join(" | ")})`);
  const tags = new Set(tips.flatMap((t) => t?.tags ?? []));
  check(board, [...tags].every((t) => ["DIV", "STRONG", "SPAN"].includes(t)), `a tooltip holds only elements the page created (${[...tags].join(",")})`);
  check(board, shown.some(hostileText("<b>bold</b> &amp; an entity")) && shown.some(hostileText('<img src=x onerror="window.pwned = 1">')) && shown.some(hostileText("<script>window.pwned = 1</script>")), "the hostile place names are in the tooltips as text");
  check(board, shown[3].includes("<img src=x onerror=1>") && shown[3].includes("<u>Double</u>"), `an axis tooltip shows the series names as text (${shown[3]})`);
  await noMarkup("with a tooltip open");

  // The table view lists the same text.
  await page.evaluate(`document.querySelectorAll(".chart .toggle").forEach((t) => t.click())`);
  const cells = await page.evaluate(`[...document.querySelectorAll(".chart .table-wrap:not([hidden]) td, .chart .table-wrap:not([hidden]) th")].map((c) => c.textContent)`);
  check(board, cells.some(hostileText('<img src=x onerror="window.pwned = 1">')) && cells.some(hostileText("<script>window.pwned = 1</script>")), `the table view shows the hostile text as text (${cells.length} cells)`);
  await noMarkup("in the table view");
  await page.evaluate(`document.querySelectorAll(".chart .toggle").forEach((t) => t.click())`);

  // Choosing a hostile option sends it as a value, and the tile follows.
  await page.evaluate(`(() => { const s = document.querySelector("select"); s.value = [...s.options].find((o) => o.value.startsWith("<script>")).value; s.dispatchEvent(new Event("change")); })()`);
  await page.waitFor(`document.querySelector(".stat .stat-value").textContent !== "0"`, "the tile to change");
  await page.waitFor(ALL_SETTLED, "cards to settle after the filter");
  const chosen = await page.evaluate(`document.querySelector(".stat .stat-value").textContent`);
  const query = await page.evaluate(`new URLSearchParams(location.search).get("place")`);
  check(board, chosen === "3" && query === "<script>window.pwned = 1</script>", `a hostile option is sent as a value and the tile follows (${chosen}, ${query})`);
  const left = await page.evaluate(`[...document.querySelectorAll(".filter-note")].filter((n) => !n.hidden).map((n) => n.textContent).join(" | ")`);
  check(board, !left.includes("not among the options"), `the note about a value in use goes away once an option is chosen (${left})`);
  await noMarkup("after choosing an option");

  // No request carried a NULL as the text "null".
  const sentNull = page.requests.filter((u) => /null/i.test(decodeURIComponent(u)));
  check(board, sentNull.length === 0, `no request carries "null" (${sentNull.join(", ")})`);
  const other = page.requests.filter((u) => new URL(u).host !== host && !u.startsWith("data:"));
  check(board, other.length === 0, `every request went to ${host} (${page.requests.length} requests)`);
  check(board, page.errors.length === 0, `no console error or CSP violation${page.errors.length ? `: ${page.errors.join(" || ")}` : ""}`);
  page.close();
  return { board, requests: page.requests.length };
}

await closedNetwork();
const browser = await startBrowser();
const results = [];
try {
  for (const board of boards) results.push(await (board === "hostile" ? hostile : smoke)(browser, board));
} finally {
  browser.close();
}
console.log(JSON.stringify(results));
if (failures.length > 0) {
  console.error(`${failures.length} check(s) failed`);
  process.exit(1);
}
