// judell/bram#386: the target-app preview pane runs inside Bram's app
// bundle, which has no NSLocationWhenInUseUsageDescription, so macOS never
// lets the pane request location -- navigator.geolocation exists but
// getCurrentPosition always fails with PERMISSION_DENIED. That lets a
// well-written app's own feature detection pass and then hit a denial
// nobody can act on. Rather than support a scenario no real user is in
// (real users open the deployed app in a browser, never this pane), make
// geolocation ABSENT here, so "geolocation" in navigator and
// if (navigator.geolocation) both fail honestly and apps take their own
// no-location path. Inserted as the first child of <head> by
// insert_preview_shim() in src-tauri/src/lib.rs so it runs before the
// page's own scripts. Self-contained, no dependencies, and never allowed
// to throw past this file -- a broken preview pane would be worse than a
// present geolocation API.
(function () {
  try {
    if (typeof location === "undefined" || location.protocol !== "bramapp:") {
      return;
    }
    try {
      delete Navigator.prototype.geolocation;
    } catch (e) {
      // Some engines refuse to delete a non-configurable accessor; the
      // "geolocation" in navigator check below decides whether a fallback
      // is still needed.
    }
    if ("geolocation" in navigator) {
      try {
        Object.defineProperty(Navigator.prototype, "geolocation", {
          get: function () {
            return undefined;
          },
          configurable: true,
        });
      } catch (e) {
        // Nothing more we can safely try; leave navigator.geolocation as-is
        // rather than throw and break the page.
      }
    }
    if (typeof console !== "undefined" && console && typeof console.info === "function") {
      console.info(
        "Bram preview: geolocation is not available in this pane. Open the app in a browser to use location."
      );
    }
  } catch (e) {
    // Never break the page over this.
  }
})();
