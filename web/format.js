// How numbers and cells are written. The locale is the browser's.

const formats = {
  integer: new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }),
  one_decimal: new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
  compact: new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }),
  plain: new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 }),
};

/** A stat's number as text. `format` is a stat format, or absent. */
export function formatNumber(value, format) {
  if (typeof value !== "number") {
    throw new Error(`expected a number, the query returned ${JSON.stringify(value)}`);
  }
  return formats[format ?? "plain"].format(value);
}

/** A cell of a table or a tooltip as text. */
export function formatCell(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "number") return formats.plain.format(value);
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

const isoDate = /^\d{4}-\d{2}-\d{2}$/;

/** `2024-01-05` as "5 January"; any other text is returned as it is. */
export function formatDay(text) {
  if (typeof text !== "string" || !isoDate.test(text)) return text;
  const [year, month, day] = text.split("-").map(Number);
  return new Date(Date.UTC(year, month - 1, day)).toLocaleDateString(undefined, {
    day: "numeric",
    month: "long",
    timeZone: "UTC",
  });
}

/** A column name as a label: `depth_km` becomes "Depth km". */
export function prettify(column) {
  const text = String(column).replaceAll("_", " ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
