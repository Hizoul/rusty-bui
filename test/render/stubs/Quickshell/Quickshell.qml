pragma Singleton
import QtQuick

// Stand-in for Quickshell's global singleton, for the headless render harness
// (test/render). Only what the shell's own Commons files call on it is here.
//
// Quickshell is not installed in the build container, so `env()` cannot read the real
// environment. That is deliberate and harmless: the theme loader then finds no theme
// files and falls back to the shell's built-in palette, which is exactly the state a
// fresh install is in.
QtObject {
  function env(name) {
    return ""
  }

  // Commons/Util spawns detached commands; the harness never wants side effects.
  function execDetached(command) {
    console.log("harness: suppressed execDetached(" + JSON.stringify(command) + ")")
  }

  function iconPath(name) {
    return ""
  }
}
