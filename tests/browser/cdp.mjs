// A small Chrome DevTools Protocol client, and the bookkeeping both scripts
// need: what the page logged, what it requested, and how to wait for it.

import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = 9222;

export async function startBrowser() {
  const process = spawn(
    "chromium",
    [
      "--headless=new",
      "--no-sandbox", // the container is the sandbox
      "--disable-gpu",
      "--disable-dev-shm-usage",
      "--hide-scrollbars",
      "--user-data-dir=/tmp/enlightenviz-profile",
      `--remote-debugging-port=${PORT}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  for (let attempt = 0; ; attempt++) {
    try {
      if ((await fetch(`http://127.0.0.1:${PORT}/json/version`)).ok) break;
    } catch {
      // not listening yet
    }
    if (attempt > 100) throw new Error("the browser did not start");
    await sleep(100);
  }
  return {
    async newPage(options) {
      const target = await (await fetch(`http://127.0.0.1:${PORT}/json/new?about:blank`, { method: "PUT" })).json();
      const page = new Page(target.webSocketDebuggerUrl);
      await page.ready;
      await page.setup(options);
      return page;
    },
    close() {
      process.kill();
    },
  };
}

export class Page {
  constructor(url) {
    this.nextId = 1;
    this.waiting = new Map();
    this.handlers = new Map();
    this.errors = []; // console errors, exceptions, failed loads, CSP violations
    this.warnings = [];
    this.requests = [];
    this.socket = new WebSocket(url);
    this.ready = new Promise((resolve, reject) => {
      this.socket.addEventListener("open", resolve);
      this.socket.addEventListener("error", () => reject(new Error("cannot reach the browser")));
    });
    this.socket.addEventListener("message", (message) => this.#receive(JSON.parse(message.data)));
  }

  #receive(message) {
    if (message.id !== undefined) {
      const { resolve, reject } = this.waiting.get(message.id);
      this.waiting.delete(message.id);
      if (message.error) reject(new Error(`${message.error.message}`));
      else resolve(message.result);
      return;
    }
    for (const handler of this.handlers.get(message.method) ?? []) handler(message.params);
  }

  send(method, params = {}) {
    const id = this.nextId++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.waiting.set(id, { resolve, reject }));
  }

  on(method, handler) {
    this.handlers.set(method, [...(this.handlers.get(method) ?? []), handler]);
  }

  async setup({ width, height, scheme, reducedMotion = false, scale = 1 }) {
    for (const domain of ["Page", "Runtime", "Log", "Network"]) await this.send(`${domain}.enable`);
    this.metrics = { width, height, deviceScaleFactor: scale, mobile: false };
    await this.send("Emulation.setDeviceMetricsOverride", this.metrics);
    await this.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-color-scheme", value: scheme },
        { name: "prefers-reduced-motion", value: reducedMotion ? "reduce" : "no-preference" },
      ],
    });
    this.on("Runtime.consoleAPICalled", ({ type, args }) => {
      const text = args.map((a) => a.value ?? a.description ?? "").join(" ");
      if (type === "error") this.errors.push(`console.error: ${text}`);
      else if (type === "warning") this.warnings.push(`console.warn: ${text}`);
    });
    this.on("Runtime.exceptionThrown", ({ exceptionDetails: d }) =>
      this.errors.push(`exception: ${d.exception?.description ?? d.text}`),
    );
    this.on("Log.entryAdded", ({ entry }) => {
      if (entry.level === "error") this.errors.push(`${entry.source}: ${entry.text} ${entry.url ?? ""}`);
    });
    this.on("Network.requestWillBeSent", ({ request }) => this.requests.push(request.url));
    this.on("Network.loadingFailed", ({ errorText, requestId }) =>
      this.errors.push(`request failed: ${errorText} (${requestId})`),
    );
  }

  async evaluate(expression) {
    const { result, exceptionDetails } = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (exceptionDetails) throw new Error(`in the page: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
    return result.value;
  }

  async waitFor(expression, what, timeout = 20000) {
    const start = Date.now();
    while (Date.now() - start < timeout) {
      if (await this.evaluate(expression)) return Date.now() - start;
      await sleep(50);
    }
    throw new Error(`timed out waiting for ${what}`);
  }

  async goto(url) {
    await this.send("Page.navigate", { url });
  }

  /**
   * The whole page as a PNG. The window is made as tall as the page first:
   * a canvas below the fold is not always painted when the shot is taken
   * "beyond the viewport".
   */
  async screenshot() {
    const height = await this.evaluate("document.documentElement.scrollHeight");
    await this.send("Emulation.setDeviceMetricsOverride", { ...this.metrics, height });
    await new Promise((resolve) => setTimeout(resolve, 1000));
    const { data } = await this.send("Page.captureScreenshot", { format: "png" });
    await this.send("Emulation.setDeviceMetricsOverride", this.metrics);
    return Buffer.from(data, "base64");
  }

  close() {
    this.socket.close();
  }
}

/** The page's cards are all out of `loading`, i.e. every query has answered. */
export const ALL_SETTLED = `(() => {
  const cards = document.querySelectorAll(".card.chart, .card.stat");
  return cards.length > 0 && [...cards].every((c) => !["loading", "refreshing"].includes(c.dataset.state));
})()`;
