// The house theme as the page's scripts see it. The colours are written once,
// as CSS custom properties in page.css (with light-dark(), so that one
// declaration holds both schemes); this file reads them back, resolved for the
// scheme in force, so that the chart libraries are themed from the same source.

const KEY = "enlightenviz-theme";
const dark = matchMedia("(prefers-color-scheme: dark)");

/** The scheme in force: the visitor's choice, else the system's. */
export function scheme() {
  return document.documentElement.dataset.theme ?? (dark.matches ? "dark" : "light");
}

/** Reads a custom property as the colour it resolves to. */
function resolved(probe, name) {
  probe.style.color = `var(${name})`;
  return getComputedStyle(probe).color;
}

/** Every colour and the font, resolved for the scheme in force. */
export function readTheme() {
  const probe = document.createElement("span");
  probe.hidden = true;
  document.body.append(probe);
  const read = (name) => resolved(probe, name);
  const theme = {
    scheme: scheme(),
    font: getComputedStyle(document.documentElement).getPropertyValue("--font").trim(),
    surface: read("--surface"),
    border: read("--border"),
    ink: read("--ink"),
    ink2: read("--ink-2"),
    muted: read("--ink-muted"),
    grid: read("--grid"),
    axis: read("--axis"),
    categorical: Array.from({ length: 8 }, (_, i) => read(`--cat-${i + 1}`)),
    ramp: Array.from({ length: 4 }, (_, i) => read(`--ramp-${i + 1}`)),
  };
  probe.remove();
  return theme;
}

/** Whether the visitor asked for less motion. */
export function reducedMotion() {
  return matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function announce() {
  document.dispatchEvent(new CustomEvent("themechange"));
}

/** Switches to the other scheme, and remembers the choice. */
export function toggleScheme() {
  const next = scheme() === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(KEY, next);
  } catch (error) {
    if (!(error instanceof DOMException)) throw error;
  }
  announce();
}

// A change of the system setting matters only while the visitor has not chosen.
dark.addEventListener("change", () => {
  if (document.documentElement.dataset.theme === undefined) announce();
});
