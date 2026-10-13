import QtQuick
import Quickshell
import "../../../app"

// Renders the weather window with one day opened up: the real app/App.qml, fed the real app
// payload, then asked for a day the way a click on a row asks for it — through the same
// `rustybui day` Process, which the stub answers from Fixture.js.
//
// A day detail cannot be reached by rendering the window alone: it only exists after a click.
// See test/render/README.md.
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
    interval: 900
    running: true
    onTriggered: {
      var rows = app.dailyRows
      console.log("harness: app view=" + (app.view ? "loaded" : "null")
        + " day rows=" + rows.length)
      if (rows.length > 1) app.openDay(rows[1])
      else console.log("harness: FAIL the payload has no second day to open")
    }
  }

  Timer {
    interval: 2000
    running: true
    onTriggered: {
      console.log("harness: day=" + (app.dayDetail ? app.dayDetail.date : "null")
        + " hours=" + (app.dayDetail ? app.dayDetail.hours.length : 0)
        + " summary=\"" + app.daySummary + "\""
        + " hint=\"" + app.dayHint + "\"")
      Harness.grabAll(driver.outDir)
    }
  }
}
