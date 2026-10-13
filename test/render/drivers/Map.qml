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
// Three images come out of it, because the map plays on its own and one grab cannot show a loop:
// where it opens (the oldest frame), where it has got to a few seconds later with the picture
// keeping up, and the far end of the extent, where the frame's age is visible.
//
// See test/render/README.md.
Window {
  id: driver

  // Declared here: Harness reads the name off the window, and a plain Window has no such
  // property of its own.
  property string harnessName: "radar-map"

  readonly property string outDir: "/work/test/render/out"

  property var radar: null
  property var radarRequest: null
  property string notice: ""

  // The host's job, mirrored from App.qml: the request the map announced, turned into the command
  // that fetches that frame. Passing the frame on matters — the answer carries the image of the
  // frame that was asked for, and a map that checks before stepping needs to be told the truth.
  readonly property var radarCommandLine: {
    if (!radarRequest) return []
    var command = ["/work/target/debug/rustybui", "radar", "--json",
      "--bbox", radarRequest.bbox.join(","),
      "--width", String(radarRequest.width),
      "--height", String(radarRequest.height)]
    if (radarRequest.timeMs) command.push("--time", new Date(Number(radarRequest.timeMs)).toISOString())
    return command
  }

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
      driver.radarRequest = { bbox: bbox, width: width, height: height, timeMs: timeMs }
      if (!radarProc.running) radarProc.running = true
    }
  }

  Process {
    id: radarProc
    command: driver.radarCommandLine

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

  // Where the map opens: it plays on its own, so the frame is the oldest one rather than the one
  // nearest now.
  Timer {
    interval: 1200
    running: true
    onTriggered: {
      console.log("harness: radar=" + (driver.radar ? driver.radar.image.path : "null")
        + " frames=" + map.frames.length + " frame=" + map.frameIndex
        + " span=" + (map.view ? map.view.spanLon.toFixed(3) : "null")
        + " basemap=" + (map.basemap ? map.basemap.runs + " runs, " + map.basemap.costMs + " ms" : "null"))
      Harness.grabAll(driver.outDir, "", false)
    }
  }

  // A few seconds into the loop: it should have moved past the frame it opened on, with the
  // picture (shownFrame) keeping up with the frame it asked for. Then the far end of the extent,
  // where the age the stepper adds to a clock is on screen.
  Timer {
    interval: 3200
    running: true
    onTriggered: {
      console.log("harness: playing=" + map.playing + " asked=" + map.frameIndex
        + " shown=" + map.shownFrame + " of " + (map.frames.length - 1))
      driver.harnessName = "radar-map-playing"
      Harness.grabAll(driver.outDir, "", false)
      if (map.frames.length) map.goToFrame(map.frames.length - 1)
      lastFrame.start()
    }
  }

  Timer {
    id: lastFrame
    interval: 500
    onTriggered: {
      console.log("harness: at the last frame, index " + map.frameIndex
        + " of " + (map.frames.length - 1))
      // Renamed rather than prefixed: the harness would staple a prefix in front of the window's
      // own name and give files like radar-map-last-radar-map.png.
      driver.harnessName = "radar-map-last"
      Harness.grabAll(driver.outDir)
    }
  }
}
