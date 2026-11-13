// Applies the visitor's saved colour choice before the page is painted, so a
// dark choice does not flash light first. A separate file, because the content
// security policy forbids inline scripts.
(() => {
  let saved = null;
  try {
    saved = localStorage.getItem("enlightenviz-theme");
  } catch (error) {
    // Storage is blocked (a privacy setting): follow the system setting. Any
    // other error is a bug and is not swallowed.
    if (!(error instanceof DOMException)) throw error;
  }
  if (saved === "light" || saved === "dark") {
    document.documentElement.dataset.theme = saved;
  }
})();
