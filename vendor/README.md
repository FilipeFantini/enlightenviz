# Vendored files

Everything the page loads at run time is embedded in the server binary
(`src/assets.rs`); nothing is fetched from another host. The libraries below
were downloaded once, inside a Docker container, from the official npm registry
at an exact version, and committed here. To check one: download the tarball,
compare its SHA-256 with the one below, and compare the file inside it.

The page loads `echarts.min.js` as a classic script, and `world-110m.json`
when a chart names the world map.

| File | Package | Version | Source | Licence |
|---|---|---|---|---|
| `echarts.min.js` | `echarts` | 6.1.0 | https://registry.npmjs.org/echarts/-/echarts-6.1.0.tgz | Apache-2.0 |
| `world-110m.json` | `world-atlas` (Natural Earth 110m land) | 2.0.2 | https://registry.npmjs.org/world-atlas/-/world-atlas-2.0.2.tgz | Natural Earth: public domain; `world-atlas`: ISC |

SHA-256 of the tarballs:

```
681aff88cc1038c3fa941a3a53d1b177a52250b05335efc607d6105ae13be481  echarts-6.1.0.tgz
032e7765f2ce00edaeafec23ff22bc3b77e42987c257da944cc585b452c05b97  world-atlas-2.0.2.tgz
```

SHA-256 of the files in this directory, and where each one comes from inside
its tarball:

```
b66b25aeb4df84e33199dc21694014d336d222cbd9deb0e5a7c14bd6aa0d0fd0  echarts.min.js          package/dist/echarts.min.js, unchanged
f8f5525d15ee0ce221365087975a3757be4620ca718b4d9950e93a497ce75907  world-110m.json         package/land-110m.json, converted (below)
```

## One file is not byte for byte

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
(ECharts), and ISC and public domain, which are permissive and ask only that the
notice is kept. The licence texts, and ECharts' `NOTICE`
file, are in `licenses/`; the minified files keep their own header comments.
