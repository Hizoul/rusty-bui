import QtQuick
import Quickshell
import Quickshell.Io
import "../../../qml" as Shared

// Renders the radar map headlessly: the real qml/RadarMap.qml, fed a synthesized frame from
// test/fixtures/radar-sample.png over the vendored basemap, inside a stub shell. The map is
// the one part of the window that does not come from the app payload — it asks for its own
// image — so it gets a driver of its own rather than riding along at the bottom of a window
// taller than the frame.
//
// See test/render/README.md.
Window {
  id: driver

  // Declared here: Harness reads the name off the window, and a plain Window has no such
  // property of its own.
  property string harnessName: "radar-map"

  readonly property string outDir: "/work/test/render/out"

  property var radar: null
  property string notice: ""

  visible: true
  color: "#101418"
  width: 620
  height: 420

  // grabToImage() captures the content item, so the window's own colour has to be painted.
  Rectangle {
    anchors.fill: parent
    color: "#101418"
  }

  Shared.RadarMap {
    id: map
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    anchors.margins: 12
    location: ({ name: "Leiden", latitude: 52.16, longitude: 4.49, countryCode: "NL" })
    radar: driver.radar
    clock24: true
    theme: ({ foreground: "#cacccc", accent: "#7aa2f7", urgent: "#f7768e" })
    notice: driver.notice

    // The map asks the host for its image; in the harness the host is this driver and the
    // Process stub answers out of Fixture.js, the same way the app's own Process does.
    onImageNeeded: function(bbox, width, height, timeMs) {
      console.log("harness: map asks for " + bbox.join(",") + " at " + width + "x" + height
        + " frame=" + (timeMs === null ? "latest" : new Date(Number(timeMs)).toISOString()))
      if (!radarProc.running) radarProc.running = true
    }
  }

  Process {
    id: radarProc
    command: ["/work/target/debug/rustybui", "radar", "--json"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          driver.radar = JSON.parse(text)
        } catch (error) {
          driver.notice = "unreadable radar payload"
        }
      }
    }
  }

  Component.onCompleted: Harness.register(driver)

  Timer {
    interval: 1200
    running: true
    onTriggered: {
      console.log("harness: radar=" + (driver.radar ? driver.radar.image.path : "null")
        + " frames=" + map.frames.length + " frame=" + map.frameIndex
        + " span=" + (map.view ? map.view.spanLon.toFixed(3) : "null")
        + " basemap=" + (map.basemap ? map.basemap.runs + " runs, " + map.basemap.costMs + " ms" : "null"))
      Harness.grabAll(driver.outDir, "", false)
      // Then walk to the far end of the extent: the map opens on the frame nearest now, where
      // the stepper says only the clock, and the age it adds is only visible further out.
      if (map.frames.length) map.goToFrame(map.frames.length - 1)
      lastFrame.start()
    }
  }

  Timer {
    id: lastFrame
    interval: 400
    onTriggered: {
      console.log("harness: at the last frame, " + map.frames.length + " of " + map.frames.length
        + ", index " + map.frameIndex)
      // Named apart from the first grab: renaming the window is cleaner than a prefix, which the
      // harness would staple in front of the window's own name.
      driver.harnessName = "radar-map-last"
      Harness.grabAll(driver.outDir)
    }
  }
}
