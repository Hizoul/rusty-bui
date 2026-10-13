import QtQuick
import Quickshell
import "../../../app"

// Presses the window's keys and photographs the list they open.
//
// The keys live in one function on the window's root precisely so this is possible: a handler that
// only exists inside `Keys.onPressed` can be photographed but never pressed, and this is the
// half of the feature a render cannot otherwise reach.
//
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
      // `?` opens the list and any key that acts on the window dismisses it; Esc backs out one
      // layer at a time. The scrolling keys walk the blocks now, and the day list moved to n/p,
      // so press both kinds and let the log say how each was handled.
      app.pressKey(Qt.Key_Question, "?")
      var listed = app.showKeys
      app.pressKey(Qt.Key_Down, "")
      app.pressKey(Qt.Key_Down, "")
      var dismissedByArrow = !app.showKeys
      var jHandled = app.pressKey(Qt.Key_J, "j")
      var kHandled = app.pressKey(Qt.Key_K, "k")
      app.pressKey(Qt.Key_N, "")
      app.pressKey(Qt.Key_N, "")
      var cursor = app.dayCursor
      app.pressKey(Qt.Key_Question, "?")
      app.pressKey(Qt.Key_M, "m")
      var dismissedByMap = !app.showKeys
      app.pressKey(Qt.Key_S, "s")
      var settings = app.showSettings
      app.pressKey(Qt.Key_Question, "?")
      app.pressKey(Qt.Key_Escape, "")
      var escapeClosesList = !app.showKeys && app.showSettings
      app.pressKey(Qt.Key_S, "s")
      app.pressKey(Qt.Key_Question, "?")

      console.log("harness: day rows=" + app.dailyRows.length
        + " · blocks=" + app.scrollStops.length
        + " · ?→listed=" + listed
        + " · ↓↓→dismissed=" + dismissedByArrow
        + " · j/k handled=" + jHandled + "/" + kHandled
        + " · nn→cursor=" + cursor
        + " · m→dismissed=" + dismissedByMap
        + " · s→settings=" + settings
        + " · Esc closes the list first: " + escapeClosesList
        + " · list open for the image: " + app.showKeys)
      Harness.grabAll(driver.outDir)
    }
  }
}
