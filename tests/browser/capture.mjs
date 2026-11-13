// Screenshots of each board, light and dark, at 1440 and 375 px wide, and the
// frames of the transition after a filter change: the baseline images in
// docs/visuals, which a change is compared against by eye. Run by run.sh
// capture.

import { writeFileSync } from "node:fs";

import { ALL_SETTLED, startBrowser } from "./cdp.mjs";

const [base, out, ...boards] = process.argv.slice(2);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function settled(page, url) {
  await page.goto(url);
  await page.waitFor(ALL_SETTLED, "every card to settle");
  await sleep(1000); // the animation ends well within a second
}

async function screenshots(browser, board) {
  for (const width of [1440, 375]) {
    for (const scheme of ["light", "dark"]) {
      const page = await browser.newPage({ width, height: 900, scheme });
      await settled(page, `${base}/boards/${board}`);
      writeFileSync(`${out}/${board}-${scheme}-${width}.png`, await page.screenshot());
      page.close();
    }
  }
}

// The frames of the first chart's card after a filter change. The browser's
// screencast pushes a frame whenever the screen changes; the frames nearest to
// 60 ms apart are kept and laid out in one strip.
async function transition(browser, board) {
  const page = await browser.newPage({ width: 1440, height: 900, scheme: "light" });
  await settled(page, `${base}/boards/${board}`);
  const rect = await page.evaluate(`(() => { const r = document.querySelector(".card.chart").getBoundingClientRect(); return {x: r.x, y: r.y, width: r.width, height: r.height}; })()`);
  const frames = [];
  page.on("Page.screencastFrame", ({ data, metadata, sessionId }) => {
    frames.push({ time: metadata.timestamp * 1000, data });
    page.send("Page.screencastFrameAck", { sessionId });
  });
  await page.send("Page.startScreencast", { format: "png", maxWidth: 1440, maxHeight: 900, everyNthFrame: 1 });
  await sleep(300);
  const before = frames.length;
  await page.evaluate(`(() => { const s = document.querySelector("select"); s.value = "5"; s.dispatchEvent(new Event("change")); })()`);
  await sleep(1200);
  await page.send("Page.stopScreencast");
  // The screen is still while the request is out; the animation starts at the
  // first frame after the longest pause that follows the click.
  const sent = frames.slice(Math.max(0, before - 1));
  const gaps = sent.slice(1).map((f, i) => Math.round(f.time - sent[i].time));
  const after = sent.slice(gaps.indexOf(Math.max(...gaps)) + 1);

  // Eight frames, as close to 60 ms apart as the screencast allows.
  const first = after[0].time;
  const chosen = [];
  for (let k = 0; k < 8; k++) {
    const wanted = first + k * 60;
    chosen.push(after.reduce((best, f) => (Math.abs(f.time - wanted) < Math.abs(best.time - wanted) ? f : best)));
  }
  const strip = await page.evaluate(`(async () => {
    const rect = ${JSON.stringify(rect)};
    const frames = ${JSON.stringify(chosen.map((f) => f.data))};
    const images = await Promise.all(frames.map((data) => new Promise((resolve) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.src = "data:image/png;base64," + data;
    })));
    const w = Math.round(rect.width), h = Math.round(rect.height);
    const canvas = document.createElement("canvas");
    canvas.width = w * 4 + 30; canvas.height = h * 2 + 10;
    const context = canvas.getContext("2d");
    context.fillStyle = "#f9f9f7";
    context.fillRect(0, 0, canvas.width, canvas.height);
    const scale = images[0].width / innerWidth;
    images.forEach((image, i) => context.drawImage(image, rect.x * scale, rect.y * scale, rect.width * scale, rect.height * scale, (i % 4) * (w + 10), Math.floor(i / 4) * (h + 10), w, h));
    return canvas.toDataURL("image/png").split(",")[1];
  })()`);
  writeFileSync(`${out}/transition-${board}.png`, Buffer.from(strip, "base64"));
  console.log(`${board}: the data arrived ${Math.max(...gaps)} ms after the click; ${after.length} frames follow, ${after.slice(1).map((f, i) => Math.round(f.time - after[i].time)).join(" ")} ms apart`);
  console.log(`${board}: frames kept at +${chosen.map((f) => Math.round(f.time - first)).join(" +")} ms`);
  page.close();
}

const browser = await startBrowser();
try {
  for (const board of boards) {
    await screenshots(browser, board);
    await transition(browser, board);
  }
} finally {
  browser.close();
}
