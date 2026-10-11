import QtQuick
import QtQuick.Window
import Quickshell
import qs.Commons
import qs.Commons as Commons
import "../../../omarchy"

// Renders the bar pill, and then the popout it opens, headlessly: the real omarchy/Pill.qml
// (which loads the real omarchy/RainPanel.qml) inside a strip of the theme's background.
// The popout is a layer surface in the shell and a window in the stub, so both end up
// grabbable. See test/render/README.md.
Window {
  id: driver

  // Declared here: Harness reads the name off the window, and a plain Window has no such
  // property of its own.
  property string harnessName: "bar-pill"

  readonly property string outDir: "/work/test/render/out"

  visible: true
  color: Commons.Color.background
  width: pill.implicitWidth + Style.space(24)
  height: Math.max(Style.space(34), pill.implicitHeight + Style.space(8))

  // grabToImage() captures the content item, so the bar strip behind the pill has to be
  // painted rather than left to the window's own colour. Declared before the pill, since
  // later siblings stack on top.
  Rectangle {
    anchors.fill: parent
    color: Commons.Color.background
  }

  Pill {
    id: pill
    anchors.centerIn: parent
    settings: ({
      binaryPath: "/work/target/debug/rustybui",
      pollMinutes: 5,
      clock24: true
    })
  }

  Component.onCompleted: Harness.register(driver)

  // First let the payload land and the pill size itself, then open the popout the way
  // the hotkey does, then grab both windows.
  Timer {
    interval: 600
    running: true
    onTriggered: {
      console.log("harness: pill visible=" + pill.visible
        + " size=" + pill.width + "x" + pill.height
        + " bar=" + (pill.bar ? "set" : "null"))
      pill.open()
    }
  }

  Timer {
    interval: 1500
    running: true
    onTriggered: {
      console.log("harness: pill visible=" + pill.visible + " popout opened=" + pill.opened)
      Harness.grabAll(driver.outDir)
    }
  }
}
