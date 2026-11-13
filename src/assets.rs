//! The files the page is made of, embedded in the binary: the page's own
//! scripts and styles (`web/`) and the vendored libraries (`vendor/`).
//!
//! Every file is served at `/assets/{hash}/{name}`, where `hash` is computed
//! at start from the names and bytes of all files together. One hash for the
//! set, not one per file, because the page's ES modules import each other by
//! relative path (`./theme.js`): a relative path keeps the hash of the module
//! that names it, so no file has to know another's hash. The cost is that a
//! release that changes any file makes a browser fetch all of them again,
//! libraries included. The files never change under one hash, so they are
//! cached for a year.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// One embedded file.
pub struct Asset {
    pub name: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

const JS: &str = "text/javascript; charset=utf-8";

/// Every file that can be served. The order is the order they are hashed in.
static FILES: [Asset; 15] = [
    Asset {
        name: "app.js",
        content_type: JS,
        bytes: include_bytes!("../web/app.js"),
    },
    Asset {
        name: "card.js",
        content_type: JS,
        bytes: include_bytes!("../web/card.js"),
    },
    Asset {
        name: "chart-echarts.js",
        content_type: JS,
        bytes: include_bytes!("../web/chart-echarts.js"),
    },
    Asset {
        name: "chart-vega.js",
        content_type: JS,
        bytes: include_bytes!("../web/chart-vega.js"),
    },
    Asset {
        name: "controls.js",
        content_type: JS,
        bytes: include_bytes!("../web/controls.js"),
    },
    Asset {
        name: "format.js",
        content_type: JS,
        bytes: include_bytes!("../web/format.js"),
    },
    Asset {
        name: "page.css",
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../web/page.css"),
    },
    Asset {
        name: "theme-init.js",
        content_type: JS,
        bytes: include_bytes!("../web/theme-init.js"),
    },
    Asset {
        name: "theme.js",
        content_type: JS,
        bytes: include_bytes!("../web/theme.js"),
    },
    Asset {
        name: "tooltip.js",
        content_type: JS,
        bytes: include_bytes!("../web/tooltip.js"),
    },
    Asset {
        name: "world.js",
        content_type: JS,
        bytes: include_bytes!("../web/world.js"),
    },
    Asset {
        name: "echarts.min.js",
        content_type: JS,
        bytes: include_bytes!("../vendor/echarts.min.js"),
    },
    Asset {
        name: "vega-interpreter.mjs",
        content_type: JS,
        bytes: include_bytes!("../vendor/vega-interpreter.mjs"),
    },
    Asset {
        name: "vega-lite.min.js",
        content_type: JS,
        bytes: include_bytes!("../vendor/vega-lite.min.js"),
    },
    Asset {
        name: "vega.min.js",
        content_type: JS,
        bytes: include_bytes!("../vendor/vega.min.js"),
    },
];

/// The world outline is data, not script.
static WORLD: Asset = Asset {
    name: "world-110m.json",
    content_type: "application/json",
    bytes: include_bytes!("../vendor/world-110m.json"),
};

pub struct Assets {
    hash: String,
}

impl Assets {
    pub fn new() -> Assets {
        // SipHash with fixed keys: the same bytes give the same hash in every
        // run of one binary, which is all a cache-busting name needs. It is
        // not a security measure.
        let mut hasher = DefaultHasher::new();
        for asset in FILES.iter().chain(std::iter::once(&WORLD)) {
            asset.name.hash(&mut hasher);
            asset.bytes.hash(&mut hasher);
        }
        Assets {
            hash: format!("{:016x}", hasher.finish()),
        }
    }

    /// The path a page uses for the file called `name`.
    pub fn path(&self, name: &str) -> String {
        format!("/assets/{}/{name}", self.hash)
    }

    /// The file at `/assets/{hash}/{name}`, if the hash is this binary's and
    /// the name is one of its files.
    pub fn get(&self, hash: &str, name: &str) -> Option<&'static Asset> {
        if hash != self.hash {
            return None;
        }
        FILES
            .iter()
            .chain(std::iter::once(&WORLD))
            .find(|a| a.name == name)
    }
}

impl Default for Assets {
    fn default() -> Assets {
        Assets::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_found_only_under_the_current_hash() {
        let assets = Assets::new();
        let path = assets.path("app.js");
        let rest = path.strip_prefix("/assets/").unwrap_or_default();
        let (hash, name) = rest.split_once('/').unwrap_or_default();
        assert_eq!(hash.len(), 16);
        assert!(assets.get(hash, name).is_some_and(|a| !a.bytes.is_empty()));
        assert!(assets.get("0000000000000000", name).is_none());
        assert!(assets.get(hash, "../Cargo.toml").is_none());
        assert!(assets.get(hash, "missing.js").is_none());
        assert!(assets.get(hash, "world-110m.json").is_some());
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = FILES.iter().map(|a| a.name).collect();
        names.push(WORLD.name);
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
