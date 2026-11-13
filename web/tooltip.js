// The tooltip's content, shared by both chart libraries: one line per entry,
// the value first and its label second. Text from data goes in with
// textContent, never as markup.

/**
 * @param {{value: string, label: string|null}[]} entries
 * A line with no label is context (a date, a place) and is drawn muted.
 */
export function tooltipContent(entries) {
  const box = document.createElement("div");
  box.className = "tip-lines";
  for (const { value, label } of entries) {
    const line = document.createElement("div");
    line.className = label === null ? "tip-line tip-context" : "tip-line";
    const strong = document.createElement("strong");
    strong.textContent = value;
    line.append(strong);
    if (label !== null) {
      const text = document.createElement("span");
      text.textContent = label;
      line.append(text);
    }
    box.append(line);
  }
  return box;
}
