// The browser smoke test, and the closed-network test of design section 8.
//
// Run by run.sh on a Docker network with no route to the internet. For each
// board: the page loads, every chart draws (a canvas with pixels in it), the
// console has no error and no CSP violation, no request goes to another host,
// a filter changes the numbers, the empty and error states are explicit, the
// table view lists the rows and the colour toggle works. Prints one JSON
// object of measurements at the end; exits 1 on the first failed check.

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

  const other = page.requests.filter((u) => new URL(u).host !== host && !u.startsWith("data:"));
  check(board, other.length === 0, `every request went to ${host} (${page.requests.length} requests${other.length ? `, others: ${other}` : ""})`);
  // The error check above provokes 400 responses, which the browser logs as
  // errors of their own; only those are expected.
  const unexpected = page.errors.filter((e) => !/status of 400/.test(e));
  check(board, unexpected.length === 0, `no console error or CSP violation${unexpected.length ? `: ${unexpected.join(" || ")}` : ""}`);
  if (page.warnings.length) console.log(`note ${board}: ${page.warnings.length} console warning(s): ${page.warnings.slice(0, 3).join(" || ")}`);
  page.close();
  return { board, firstChart: Math.round(firstChart), requests: page.requests.length };
}

await closedNetwork();
const browser = await startBrowser();
const results = [];
try {
  for (const board of boards) results.push(await smoke(browser, board));
} finally {
  browser.close();
}
console.log(JSON.stringify(results));
if (failures.length > 0) {
  console.error(`${failures.length} check(s) failed`);
  process.exit(1);
}
