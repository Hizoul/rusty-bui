import QtQuick
import Quickshell
import "../../../app"

// Renders the standalone weather window headlessly: the real app/App.qml, fed the real
// payload the binary produces, inside a stub shell. See test/render/README.md.
Item {
  id: driver

  // The harness always runs inside the build container, where the repo is /work.
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
    interval: 900
    running: true
    onTriggered: {
      console.log("harness: app view=" + (app.view ? "loaded" : "null")
        + " error=\"" + app.errorText + "\" hint=\"" + app.hintText + "\"")
      Harness.grabAll(driver.outDir)
    }
  }
}
