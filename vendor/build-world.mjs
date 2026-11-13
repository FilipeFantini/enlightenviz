// Turns world-atlas' land-110m.json (TopoJSON, Natural Earth 110m, public
// domain) into one GeoJSON FeatureCollection with the coordinates rounded to
// two decimals (about 1 km, far finer than a 110m outline), and without
// Antarctica (every polygon that lies wholly south of 60 degrees south): it
// takes a fifth of the map's height and no board draws events there. Both
// chart libraries read the result. Usage: node build-world.mjs <land-110m.json>
// The steps and the checksums are in README.md.
import { readFileSync } from "node:fs";

const topology = JSON.parse(readFileSync(process.argv[2], "utf8"));
const { scale, translate } = topology.transform;
const round = (n) => Math.round(n * 100) / 100;

// Arcs are stored as integer deltas; accumulate, then scale and translate.
const arcs = topology.arcs.map((arc) => {
  let x = 0;
  let y = 0;
  return arc.map(([dx, dy]) => {
    x += dx;
    y += dy;
    return [round(x * scale[0] + translate[0]), round(y * scale[1] + translate[1])];
  });
});

// A ring is a list of arc indexes; a negative index ~i is arc i reversed.
// Consecutive arcs share their joint point, which is written once.
function ring(indexes) {
  const points = [];
  for (const index of indexes) {
    const arc = index >= 0 ? arcs[index] : arcs[~index].slice().reverse();
    points.push(...(points.length ? arc.slice(1) : arc));
  }
  return points;
}

// A planar map draws a straight line between two points, so a ring that crosses
// the antimeridian (Chukotka, Fiji) would be closed by a line across the whole
// map. Make its longitudes continuous, then cut it at 180 degrees and bring
// the part beyond back by 360. Holes of such a ring are dropped; none of the
// 110m outline's holes lies there.
function unwrap(points) {
  const out = [points[0]];
  for (const [longitude, latitude] of points.slice(1)) {
    let value = longitude;
    const previous = out[out.length - 1][0];
    while (value - previous > 180) value -= 360;
    while (previous - value > 180) value += 360;
    out.push([value, latitude]);
  }
  return out;
}

function clip(ring, edge, keep) {
  const open = ring.slice(0, -1);
  const out = [];
  open.forEach((a, i) => {
    const b = open[(i + 1) % open.length];
    if (keep(a[0])) out.push(a);
    if (keep(a[0]) !== keep(b[0])) {
      const t = (edge - a[0]) / (b[0] - a[0]);
      out.push([edge, a[1] + t * (b[1] - a[1])]);
    }
  });
  return out.length > 0 ? [...out, out[0]] : out;
}

function cut(polygon) {
  const outer = unwrap(polygon[0]);
  const longitudes = outer.map(([x]) => x);
  const [low, high] = [Math.min(...longitudes), Math.max(...longitudes)];
  const shift = (ring, by) => ring.map(([x, y]) => [x + by, y]);
  if (high > 180) {
    return [[clip(outer, 180, (x) => x <= 180)], [shift(clip(outer, 180, (x) => x >= 180), -360)]];
  }
  if (low < -180) {
    return [[clip(outer, -180, (x) => x >= -180)], [shift(clip(outer, -180, (x) => x <= -180), 360)]];
  }
  return [polygon];
}

function polygons(geometry) {
  if (geometry.type === "Polygon") return [geometry.arcs.map(ring)];
  if (geometry.type === "MultiPolygon") return geometry.arcs.map((p) => p.map(ring));
  throw new Error(`unexpected geometry type ${geometry.type}`);
}

const land = topology.objects.land;
const parts = land.type === "GeometryCollection" ? land.geometries : [land];
const south = (polygon) => polygon[0].every(([, latitude]) => latitude < -60);
const features = [
  {
    type: "Feature",
    properties: { name: "land" },
    geometry: { type: "MultiPolygon", coordinates: parts
        .flatMap(polygons)
        .filter((p) => !south(p))
        .flatMap(cut)
        .map((p) => p.map((ring) => ring.map(([x, y]) => [round(x), round(y)]))) },
  },
];
process.stdout.write(JSON.stringify({ type: "FeatureCollection", features }));
