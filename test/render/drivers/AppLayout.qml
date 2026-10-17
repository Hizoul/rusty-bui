import QtQuick
import Quickshell
import "../../../app"

// Moves the window's cards the three ways a person can — Alt+↓, dragging a grip, and the
// show/hide chips — and photographs the result with a stored layout and the words duration style.
//
// The order is a list the window keeps and the binary stores, so the moves are checked on the
// list itself, and the picture is the layout a stored preference produces.
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
      var start = app.cardOrder.join(",")

      // Alt+↓ moves the card the view is resting on (the first, at the top) one place down.
      var altHandled = app.pressKey(Qt.Key_Down, "", Qt.AltModifier)
      var afterAlt = app.cardOrder.join(",")

      // A drag: pick up the last card and drop it at the very top of the stack.
      app.beginDrag("air", app.stackHeight())
      app.dragTo(0)
      var held = app.dragId
      var midDrag = app.cardOrder.join(",")
      var airTop = app.stackY(app.cardItems.air[0])
      app.endDrag()
      var afterDrag = app.cardOrder.join(",")

      // Hiding takes a card out of the stack and out of the keys' way.
      var stopsBefore = app.scrollStops.length
      app.setCardHidden("map", true)
      var stopsAfter = app.scrollStops.length
      var mapShown = app.cardShown("map")

      console.log("harness: start=" + start
        + " · alt handled=" + altHandled + " → " + afterAlt
        + " · drag held=" + held + " mid=" + midDrag + " airTop=" + airTop + " → " + afterDrag
        + " · stops " + stopsBefore + "→" + stopsAfter + " · map shown=" + mapShown)

      // The stored layout the next start would read, and the duration style in force.
      app.prefs = {
        clock24: true, hours: 24, offline: false, units: "metric",
        durationStyle: "words",
        cardOrder: ["nowcast", "now", "sky", "hours", "days", "air", "map"],
        hiddenCards: ["map"],
        stored: { clock: false, hours: false, offline: false, units: false, duration: true, cards: true },
        location: { name: "Leiden", latitude: 52.16, longitude: 4.49, country_code: "NL", timezone: "Europe/Amsterdam" },
        places: [], placeLimit: 12
      }
      app.layoutOrder = null
      app.layoutHidden = null
      app.showSettings = true
      Qt.callLater(function() { Harness.grabAll(driver.outDir) })
    }
  }
}
