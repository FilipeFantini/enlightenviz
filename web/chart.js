// Draws a chart with Apache ECharts, under the house theme.
//
// What is native to ECharts and what is written here: ECharts animates a
// change of data by itself (setOption with new rows), draws the legend, the
// tooltip box and the hit testing, and highlights the mark a tooltip is shown
// for. This file adds the house theme (merged under the author's option, so
// the author wins), the rows, the world outline, a tooltip whose text is
// built with textContent, and keyboard stepping through the values.

import { formatCell, formatDay, prettify } from "./format.js";
import { tooltipContent } from "./tooltip.js";
import { aspect, loadWorld, usesWorld } from "./world.js";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

/** Deep merge of plain objects; arrays and scalars of `over` replace `base`. */
function merge(base, over) {
  if (!isObject(base) || !isObject(over)) return over === undefined ? base : over;
  const out = { ...base };
  for (const key of Object.keys(over)) out[key] = key in base ? merge(base[key], over[key]) : over[key];
  return out;
}

const asList = (value) => (value === undefined ? [] : Array.isArray(value) ? value : [value]);
const first = (value) => (Array.isArray(value) ? value[0] : value);

/** Space kept around a map, so that a point at its edge is not cut by the card. */
export const MAP_MARGIN = 8;

/** Every `map` the option names must be the world outline the page registers. */
function checkMaps(node) {
  if (Array.isArray(node)) return node.forEach(checkMaps);
  if (!isObject(node)) return;
  for (const [key, value] of Object.entries(node)) {
    if (key === "map" && typeof value === "string" && value !== "world") {
      throw new Error(`the map \`${value}\` does not exist: the only map is \`world\``);
    }
    checkMaps(value);
  }
}

function axisDefaults(axis, theme) {
  const value = axis.type === "value" || axis.type === "log";
  return merge(
    {
      // The labels of an axis whose scale changes are redrawn at once. Animated,
      // the old labels and the new ones are on the screen together for a frame
      // and overlap; the marks still move.
      animation: false,
      axisLine: { lineStyle: { color: theme.axis, width: 1 } },
      axisTick: { show: false },
      axisLabel: { color: theme.muted, fontSize: 12, fontFamily: theme.font, hideOverlap: true },
      splitLine: value ? { show: true, lineStyle: { color: theme.grid, width: 1, type: "solid" } } : { show: false },
    },
    axis,
  );
}

// While a mark is hovered it is at full strength and the others in the chart
// recede.
const emphasis = { focus: "self", blurScope: "coordinateSystem" };
const blur = { itemStyle: { opacity: 0.3 }, label: { opacity: 0.3 } };

function seriesDefaults(series, theme, horizontal) {
  switch (series.type) {
    case "bar":
      return merge(
        {
          barMaxWidth: 24,
          label: { color: theme.ink2, fontSize: 12, fontFamily: theme.font, textBorderWidth: 0 },
          itemStyle: {
            borderColor: theme.surface,
            borderWidth: 2,
            borderRadius: horizontal ? [0, 4, 4, 0] : [4, 4, 0, 0],
          },
          emphasis,
          blur,
          animationDelay: (index) => Math.min(index * 8, 240),
        },
        series,
      );
    case "scatter":
      // Dense points must stay readable: translucent, ringed in the surface
      // colour, and full strength when hovered.
      return merge(
        {
          itemStyle: { borderColor: theme.surface, borderWidth: 1, opacity: 0.55 },
          emphasis: { ...emphasis, itemStyle: { opacity: 1, borderWidth: 1 } },
          blur,
          animationDelay: (index) => Math.min(index * 3, 240),
        },
        series,
      );
    case "line":
      return merge(
        {
          lineStyle: { width: 2, join: "round", cap: "round" },
          symbolSize: 8,
          itemStyle: { borderColor: theme.surface, borderWidth: 2 },
          areaStyle: series.areaStyle === undefined ? undefined : { opacity: 0.1 },
        },
        series,
      );
    default:
      return series;
  }
}

/**
 * A scatter's points are small, and a hit area under 24 px is hard to find with
 * a pointer. Each scatter series therefore gets a twin: invisible, 24 px, drawn
 * above, which takes the pointer and the tooltip (the series itself is silent).
 * Returns the twins, and which series each one stands for.
 */
function withHitAreas(series) {
  const twins = [];
  const hitFor = new Map();
  for (const [index, s] of series.entries()) {
    if (s.type !== "scatter") continue;
    s.silent = true;
    const twin = {
      type: "scatter",
      coordinateSystem: s.coordinateSystem,
      geoIndex: s.geoIndex,
      encode: s.encode,
      symbolSize: 24,
      itemStyle: { opacity: 0 },
      emphasis: { disabled: true },
      blur: { itemStyle: { opacity: 0 } },
      z: (s.z ?? 2) + 1,
    };
    hitFor.set(series.length + twins.length, index);
    twins.push(twin);
  }
  series.push(...twins);
  return hitFor;
}

/** The author's option with the house theme and the rows. */
function houseOption(authored, theme, palette, rows, motion) {
  checkMaps(authored);
  // ECharts changes the objects it is given; the author's option must stay as written.
  const spec = structuredClone(authored);
  const horizontal = asList(spec.yAxis)[0]?.type === "category";
  const series = asList(spec.series).map((s) => seriesDefaults(s, theme, horizontal));
  const hitFor = withHitAreas(series);
  // Only named series are in the legend: an unnamed one is a helper, such as
  // the invisible hit area drawn over a scatter.
  const legend = series.filter((s) => s.name !== undefined).length >= 2;
  const option = merge(
    {
      backgroundColor: "transparent",
      color: palette === "ordered" ? theme.ramp : theme.categorical,
      textStyle: { fontFamily: theme.font, color: theme.ink2 },
      animation: motion,
      animationDuration: 600,
      animationDurationUpdate: 300,
      animationEasing: "cubicOut",
      animationEasingUpdate: "cubicOut",
      grid: { left: 0, right: 12, top: legend ? 36 : 12, bottom: 0, containLabel: true },
      legend: {
        show: legend,
        top: 0,
        left: 0,
        icon: "roundRect",
        itemWidth: 10,
        itemHeight: 10,
        itemGap: 16,
        textStyle: { color: theme.ink2, fontSize: 12, fontFamily: theme.font },
      },
      tooltip: {
        confine: true,
        appendToBody: true,
        backgroundColor: theme.surface,
        borderColor: theme.border,
        borderWidth: 1,
        padding: [8, 10],
        extraCssText: "border-radius: 8px; box-shadow: none;",
        textStyle: { color: theme.ink, fontSize: 13, fontFamily: theme.font },
        axisPointer: { type: "none" },
      },
    },
    spec,
  );
  option.dataset = { source: rows };
  option.series = series;
  for (const key of ["xAxis", "yAxis"]) {
    if (spec[key] !== undefined) {
      option[key] = Array.isArray(spec[key])
        ? spec[key].map((axis) => axisDefaults(axis, theme))
        : axisDefaults(spec[key], theme);
    }
  }
  if (option.geo !== undefined) {
    option.geo = merge(
      {
        // 1: longitude and latitude are drawn at the same scale (equirectangular).
        aspectScale: 1,
        silent: true,
        itemStyle: { areaColor: theme.grid, borderWidth: 0 },
        emphasis: { disabled: true },
      },
      option.geo,
    );
  }
  return { option, hitFor };
}

/** The text of a tooltip, as value-first entries, from what ECharts reports. */
function entriesFor(params, series) {
  const list = Array.isArray(params) ? params : [params];
  const row = list[0].data;
  const entries = [];
  if (Array.isArray(params)) {
    for (const p of list) {
      const dimension = valueDimension(series[p.seriesIndex], p.data);
      entries.push({ value: formatCell(p.data[dimension]), label: p.seriesName });
    }
    const x = first(series[list[0].seriesIndex]?.encode?.x);
    if (typeof x === "string") entries.push({ value: formatDay(row[x]), label: null });
    return entries;
  }
  const encode = series[params.seriesIndex]?.encode ?? {};
  const named = asList(encode.tooltip);
  const dimensions = named.length > 0 ? named : [valueDimension(series[params.seriesIndex], row), first(encode.y), first(encode.x)];
  const seen = new Set();
  for (const dimension of dimensions) {
    if (typeof dimension !== "string" || seen.has(dimension) || !(dimension in row)) continue;
    seen.add(dimension);
    const label = dimension === dimensions[0] && params.seriesName && named.length === 0 ? params.seriesName : prettify(dimension);
    entries.push({ value: formatDay(formatCell(row[dimension])), label });
  }
  return entries;
}

/** The column that holds the number a mark stands for. */
function valueDimension(series, row) {
  const encode = series?.encode ?? {};
  if (encode.value !== undefined) return first(encode.value);
  const candidates = [first(encode.y), first(encode.x)];
  return candidates.find((name) => typeof row[name] === "number") ?? candidates[0];
}

/**
 * @param {HTMLElement} host
 * @param {{spec: object, palette: string}} def
 * @param {{theme: object, motion: boolean}} context
 */
export async function create(host, def, { theme, motion }) {
  let shape = null; // width over height of the world outline, when the chart draws it
  if (usesWorld(def.spec)) {
    const world = await loadWorld();
    echarts.registerMap("world", world);
    shape = aspect(world);
  }
  const chart = echarts.init(host, null, { renderer: "canvas" });
  // ECharts stretches a map to the box it is given, so the box that fits the
  // chart while keeping the outline's proportions is worked out here.
  const mapBox = () => {
    // A margin keeps a point at the edge of the map (Tonga, 176 degrees west)
    // from being cut by the edge of the card.
    const margin = MAP_MARGIN;
    const { width: full, height: tall } = host.getBoundingClientRect();
    const [width, height] = [full - 2 * margin, tall - 2 * margin];
    const [w, h] = [Math.min(width, height * shape), Math.min(height, width / shape)];
    const [x, y] = [margin + Math.floor((width - w) / 2), margin + Math.floor((height - h) / 2)];
    return { left: x, right: x, top: y, bottom: y };
  };
  let option = null;
  let hitFor = new Map();
  let marks = []; // every mark a tooltip can be shown for, left to right
  let focused = -1;

  /** Shows the tooltip of mark `index` and emphasises the mark. */
  function show(index) {
    if (marks.length === 0) return;
    focused = Math.min(marks.length - 1, Math.max(0, index));
    const { seriesIndex, dataIndex } = marks[focused];
    const twin = [...hitFor].find(([, original]) => original === seriesIndex)?.[0];
    chart.dispatchAction({ type: "downplay" });
    chart.dispatchAction({ type: "highlight", seriesIndex, dataIndex });
    chart.dispatchAction({ type: "showTip", seriesIndex: twin ?? seriesIndex, dataIndex });
  }

  return {
    /** Draws `rows`; ECharts animates the change by itself. */
    update(rows, { animate = true } = {}) {
      focused = -1;
      if (option === null) {
        ({ option, hitFor } = houseOption(def.spec, theme, def.palette, rows, motion && animate));
        if (option.geo !== undefined) Object.assign(option.geo, mapBox());
        option.tooltip.formatter = (params) => tooltipContent(entriesFor(params, option.series));
        chart.setOption(option);
        // The pointer is over a twin; the mark it stands for is emphasised.
        chart.on("mouseover", "series", ({ seriesIndex, dataIndex }) => {
          if (hitFor.has(seriesIndex)) chart.dispatchAction({ type: "highlight", seriesIndex: hitFor.get(seriesIndex), dataIndex });
        });
        chart.on("mouseout", "series", ({ seriesIndex, dataIndex }) => {
          if (hitFor.has(seriesIndex)) chart.dispatchAction({ type: "downplay", seriesIndex: hitFor.get(seriesIndex), dataIndex });
        });
      } else {
        chart.setOption({ animation: motion && animate, dataset: { source: rows } });
      }
      marks = [];
      for (let dataIndex = 0; dataIndex < rows.length; dataIndex++) {
        for (const [seriesIndex, s] of option.series.entries()) {
          if (!hitFor.has(seriesIndex) && s.type !== undefined) marks.push({ seriesIndex, dataIndex });
        }
      }
    },
    /**
     * Fits the chart to its box. Resolves to true when it must be built again:
     * a map's box changes with the card's, and ECharts does not move the
     * points of a scatter when the map under them is laid out again.
     */
    resize() {
      chart.resize();
      if (option?.geo === undefined) return false;
      const box = mapBox();
      return ["left", "right", "top", "bottom"].some((side) => box[side] !== option.geo[side]);
    },
    /** Shows the tooltip of the next (`1`) or previous (`-1`) mark. */
    step(delta) {
      show(focused < 0 ? (delta > 0 ? 0 : marks.length - 1) : focused + delta);
    },
    stepTo(index) {
      show(index);
    },
    clear() {
      focused = -1;
      chart.dispatchAction({ type: "hideTip" });
      chart.dispatchAction({ type: "downplay" });
    },
    dispose() {
      chart.dispose();
    },
  };
}
