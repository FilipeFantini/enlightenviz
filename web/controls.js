// The filter row: one control per filter, and the filters' state in the URL's
// query string, so that a link reproduces the view.
//
// A value is kept as text, as it travels in the query string: a string for
// a single value, an array of strings for a list.

const isoDate = /^\d{4}-\d{2}-\d{2}$/;

const sameValue = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/** The starting value of a filter: the parameter's default, as text. */
export function defaultValue(filter) {
  const value = filter.default;
  return Array.isArray(value) ? value.map(String) : String(value);
}

/**
 * The state the URL asks for, with the default where it says nothing.
 * `options` maps a param to its options (`{value, label}`), or to null when
 * they could not be loaded. A value that is not valid is not used, and is
 * reported in `notices` so that the page says so.
 */
export function stateFromUrl(filters, search, options) {
  const query = new URLSearchParams(search);
  const values = {};
  const notices = [];
  for (const filter of filters) {
    const given = query.getAll(filter.param);
    let value = defaultValue(filter);
    if (given.length > 0) {
      const known = options[filter.param]?.map((o) => o.value);
      let problem = null;
      if (filter.control === "date") {
        if (given.length !== 1 || !isoDate.test(given[0])) problem = "it is not a date written YYYY-MM-DD";
      } else if (filter.control === "select" && given.length !== 1) {
        problem = "it must be given once";
      } else if (known !== undefined && !given.every((v) => known.includes(v))) {
        problem = "it is not one of the options";
      }
      if (problem === null) value = filter.control === "multiselect" ? given : given[0];
      else notices.push(`The link sets ${filter.label} to ${given.join(", ")}, but ${problem}; the default is used.`);
    }
    values[filter.param] = value;
  }
  return { values, notices };
}

/** Writes the values that differ from their defaults into the address bar. */
export function writeUrl(filters, values) {
  const query = new URLSearchParams();
  for (const filter of filters) {
    if (sameValue(values[filter.param], defaultValue(filter))) continue;
    for (const item of [].concat(values[filter.param])) query.append(filter.param, item);
  }
  const text = query.toString();
  history.replaceState(null, "", text === "" ? location.pathname : `${location.pathname}?${text}`);
}

/** A line of small text under a control. Text only, never markup. */
function note() {
  const line = document.createElement("p");
  line.className = "filter-note";
  line.hidden = true;
  return line;
}

/**
 * Says so, at the control, when the value in use is not among the options: a
 * default, say, that the options query did not return. Without it the control
 * would be blank while the charts used the value.
 */
function missingNote(line, options, current) {
  const known = options.map((o) => o.value);
  const missing = [].concat(current).filter((v) => !known.includes(v));
  line.hidden = missing.length === 0;
  line.textContent = missing.length === 0 ? "" : `In use, but not among the options: ${missing.join(", ")}.`;
}

/** Says that some options had no value and are not offered. */
function skippedNote(line, skipped) {
  line.hidden = !skipped;
  line.textContent = skipped ? `${skipped} option${skipped === 1 ? " has" : "s have"} no value and ${skipped === 1 ? "is" : "are"} not shown.` : "";
}

function field(filter, control) {
  const wrapper = document.createElement("div");
  wrapper.className = "filter";
  const label = document.createElement("label");
  label.textContent = filter.label;
  label.className = "filter-label";
  wrapper.append(label, control);
  return { wrapper, label };
}

function select(filter, options, values, change, skipped) {
  const control = document.createElement("select");
  for (const option of options) {
    const item = document.createElement("option");
    item.value = option.value;
    item.textContent = option.label;
    control.append(item);
  }
  control.value = values[filter.param];
  const missing = note();
  const gone = note();
  skippedNote(gone, skipped);
  missingNote(missing, options, values[filter.param]);
  control.addEventListener("change", () => {
    change(control.value);
    missingNote(missing, options, values[filter.param]);
  });
  const { wrapper, label } = field(filter, control);
  wrapper.append(missing, gone);
  label.htmlFor = control.id = `filter-${filter.param}`;
  return wrapper;
}

function date(filter, values, change) {
  const control = document.createElement("input");
  control.type = "date";
  control.required = true;
  control.value = values[filter.param];
  control.addEventListener("change", () => {
    // A cleared date is no value: put the last one back rather than guess.
    if (control.value === "") control.value = values[filter.param];
    else change(control.value);
  });
  const { wrapper, label } = field(filter, control);
  label.htmlFor = control.id = `filter-${filter.param}`;
  return wrapper;
}

function multiselect(filter, options, values, change, skipped) {
  const details = document.createElement("details");
  details.className = "multi";
  const summary = document.createElement("summary");
  const menu = document.createElement("div");
  menu.className = "menu";
  const boxes = options.map((option) => {
    const row = document.createElement("label");
    row.className = "check";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.value = option.value;
    box.checked = values[filter.param].includes(option.value);
    const text = document.createElement("span");
    text.textContent = option.label;
    row.append(box, text);
    menu.append(row);
    return box;
  });
  const describe = () => {
    const chosen = options.filter((o) => values[filter.param].includes(o.value));
    summary.textContent =
      chosen.length === options.length
        ? `All (${options.length})`
        : chosen.length <= 2
          ? chosen.map((o) => o.label).join(", ")
          : `${chosen.length} of ${options.length}`;
    // An empty list cannot be sent in a query string, and would fall back to
    // the parameter's default: so the last choice cannot be removed.
    for (const box of boxes) box.disabled = chosen.length === 1 && box.checked;
  };
  const missing = note();
  const gone = note();
  skippedNote(gone, skipped);
  missingNote(missing, options, values[filter.param]);
  for (const box of boxes) {
    box.addEventListener("change", () => {
      change(boxes.filter((b) => b.checked).map((b) => b.value));
      describe();
      missingNote(missing, options, values[filter.param]);
    });
  }
  details.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && details.open) {
      details.open = false;
      summary.focus();
    }
  });
  document.addEventListener("click", (event) => {
    if (!details.contains(event.target)) details.open = false;
  });
  describe();
  details.append(summary, menu);
  const { wrapper, label } = field(filter, details);
  wrapper.append(missing, gone);
  // A label cannot name a <details>; name the summary instead.
  label.removeAttribute("for");
  label.id = `filter-${filter.param}-label`;
  summary.setAttribute("aria-labelledby", `${label.id}`);
  return wrapper;
}

/**
 * Fills `container` with the controls. `skipped` counts, by param, the options
 * that had no value (a NULL) and are not offered. `change(param, value)` is
 * called with the new value of a filter after the visitor changes it.
 */
export function buildControls(container, filters, options, skipped, values, change) {
  for (const filter of filters) {
    const list = options[filter.param];
    const set = (value) => {
      values[filter.param] = value;
      change(filter.param, value);
    };
    if (filter.control === "date") {
      container.append(date(filter, values, set));
    } else if (list === null) {
      const missing = document.createElement("div");
      missing.className = "filter";
      const label = document.createElement("span");
      label.className = "filter-label";
      label.textContent = filter.label;
      const note = document.createElement("span");
      note.className = "filter-error";
      note.textContent = "Options could not be loaded";
      missing.append(label, note);
      container.append(missing);
    } else if (filter.control === "select") {
      container.append(select(filter, list, values, set, skipped[filter.param] ?? 0));
    } else {
      container.append(multiselect(filter, list, values, set, skipped[filter.param] ?? 0));
    }
  }
}
