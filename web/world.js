// The world outline: the one static dataset a chart may use. The page fetches
// it from its own origin, once, and only for a chart that names it.

let loading = null;

/** The outline as a GeoJSON FeatureCollection. */
export function loadWorld() {
  loading ??= fetch(new URL("./world-110m.json", import.meta.url)).then((response) => {
    if (!response.ok) throw new Error(`the world outline could not be loaded (${response.status})`);
    return response.json();
  });
  return loading;
}

/** Whether a specification refers to the world outline. */
export function usesWorld(spec) {
  return JSON.stringify(spec).includes('"world"');
}

/** Width over height of the outline drawn with longitude and latitude at one scale. */
export function aspect(world) {
  let [west, east, south, north] = [Infinity, -Infinity, Infinity, -Infinity];
  for (const feature of world.features) {
    for (const polygon of feature.geometry.coordinates) {
      for (const [x, y] of polygon[0]) {
        [west, east] = [Math.min(west, x), Math.max(east, x)];
        [south, north] = [Math.min(south, y), Math.max(north, y)];
      }
    }
  }
  return (east - west) / (north - south);
}
