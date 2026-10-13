import QtQuick
import Quickshell
import "../../../app"

// Renders the window with its settings view open, so the location search and the display
// toggles can be looked at. Same app, different view: see test/render/README.md.
Item {
  id: driver

  readonly property string outDir: "/work/test/render/out"

  App {
    id: app
    settings: ({
      binaryPath: "/work/target/debug/rustybui",
      refreshMinutes: 10,
      hours: 24,
      clock24: true,
      offline: false
    })
  }

  Component.onCompleted: app.open("{}")

  Timer {
    interval: 700
    running: true
    onTriggered: {
      app.showSettings = true
      // A search the user would have typed, so the results list has something in it.
      app.searchQuery = "Leiden"
      app.runSearch()
    }
  }

  Timer {
    interval: 1400
    running: true
    onTriggered: {
      console.log("harness: settings open=" + app.showSettings
        + " results=" + app.searchResults.length
        + " prefs=" + (app.prefs ? "loaded" : "null"))
      Harness.grabAll(driver.outDir, "settings")
    }
  }
}
