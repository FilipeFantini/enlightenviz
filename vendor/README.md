# Vendored files

Everything the page loads at run time is embedded in the server binary
(`src/assets.rs`); nothing is fetched from another host. The libraries below
were downloaded once, inside a Docker container, from the official npm registry
at an exact version, and committed here. To check one: download the tarball,
compare its SHA-256 with the one below, and compare the file inside it.

Which libraries a board needs is decided by its charts: a board with
`vega_lite` charts loads Vega and Vega-Lite, one with `echarts` charts loads
ECharts, and neither loads the other. While the two are compared (see
`docs/review/chart-library/`), both are here; the one that is not chosen is
removed.

| File | Package | Version | Source | Licence |
|---|---|---|---|---|
| `echarts.min.js` | `echarts` | 6.1.0 | https://registry.npmjs.org/echarts/-/echarts-6.1.0.tgz | Apache-2.0 |
| `vega.min.js` | `vega` | 6.4.0 | https://registry.npmjs.org/vega/-/vega-6.4.0.tgz | BSD-3-Clause |
| `vega-lite.min.js` | `vega-lite` | 6.5.0 | https://registry.npmjs.org/vega-lite/-/vega-lite-6.5.0.tgz | BSD-3-Clause |
| `vega-interpreter.mjs` | `vega-interpreter` | 2.3.2 | https://registry.npmjs.org/vega-interpreter/-/vega-interpreter-2.3.2.tgz | BSD-3-Clause |
| `world-110m.json` | `world-atlas` (Natural Earth 110m land) | 2.0.2 | https://registry.npmjs.org/world-atlas/-/world-atlas-2.0.2.tgz | Natural Earth: public domain; `world-atlas`: ISC |

SHA-256 of the tarballs:

```
681aff88cc1038c3fa941a3a53d1b177a52250b05335efc607d6105ae13be481  echarts-6.1.0.tgz
3521fb6ff68a94a40323954f0ba4484ba4014b157bfd37f89a3b063082320c42  vega-6.4.0.tgz
811230a6bfd2cdbfc359d0c38f624a90df86657b7f251b67be88bcc4eee32b7f  vega-lite-6.5.0.tgz
b83489633c1320e9d6f839781f0c84c661a5ca2666fe38cb83f7c52a531fffec  vega-interpreter-2.3.2.tgz
032e7765f2ce00edaeafec23ff22bc3b77e42987c257da944cc585b452c05b97  world-atlas-2.0.2.tgz
```

SHA-256 of the files in this directory, and where each one comes from inside
its tarball:

```
b66b25aeb4df84e33199dc21694014d336d222cbd9deb0e5a7c14bd6aa0d0fd0  echarts.min.js          package/dist/echarts.min.js, unchanged
8f6a3587cf8d4f42c7e08120e3eb05d067e746d554e39d2dcf52acc0bd5ba28f  vega.min.js             package/build/vega.min.js, unchanged
83e215e84e308ee8688fa6503ba0f1cf8a3c05c0d61df2841a0b92bf86809e00  vega-lite.min.js        package/build/vega-lite.min.js, unchanged
ca1de8c6b88d24f30bb3b576dbf9562ccf3899f25c421c3a0a88dccc7bc65822  vega-interpreter.mjs    package/build/vega-interpreter.js, first line changed (below)
f8f5525d15ee0ce221365087975a3757be4620ca718b4d9950e93a497ce75907  world-110m.json         package/land-110m.json, converted (below)
```

## Two files are not byte for byte

**`vega-interpreter.mjs`** is the expression interpreter Vega needs under a
content security policy without `unsafe-eval`: Vega's own expression compiler
builds code from text, which the policy forbids, and the interpreter evaluates
the same expressions as trees. The package ships only an ES module whose first
line is `import { ascending, isString, DisallowedObjectProperties } from 'vega-util';`.
A browser cannot resolve that bare name without a bundler or an import map (an
import map is inline script, which the policy forbids), and the same functions
are already in `vega.min.js`, so the first line was replaced by

```js
const { ascending, isString, DisallowedObjectProperties } = globalThis.vega;
```

(`sed '1s#.*#...#'`; the rest of the file is as published.) It must therefore
load after `vega.min.js`, which the page's order of scripts guarantees. A new
version of the package needs the same one-line change and a check that its first
line is still that import.

**`world-110m.json`** is `land-110m.json` (TopoJSON, quantised) turned into one
GeoJSON `FeatureCollection` by `build-world.mjs`: the coordinates are decoded
and rounded to two decimals (about 1 km), polygons wholly south of 60 degrees
south (Antarctica) are dropped, and polygons that cross the antimeridian
(Chukotka, Fiji) are cut at 180 degrees, because a planar map would otherwise
close them with a line across the whole map. It is 66 KB (23 KB gzipped).
To reproduce it, with `land-110m.json` extracted from the tarball:

```sh
docker run --rm -v "$PWD:/w" alpine:3.24 sh -c 'apk add -q nodejs && node /w/vendor/build-world.mjs /w/land-110m.json' > vendor/world-110m.json
```

## Licences

All are compatible with this project's Apache-2.0 licence: Apache-2.0 itself
(ECharts), and BSD-3-Clause, ISC and public domain, which are permissive and
ask only that the notice is kept. The licence texts, and ECharts' `NOTICE`
file, are in `licenses/`; the minified files keep their own header comments.
