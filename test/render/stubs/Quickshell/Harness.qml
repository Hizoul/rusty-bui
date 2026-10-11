pragma Singleton
import QtQuick

// Registry for the render harness: every stub window registers itself, and the driver
// calls grabAll() once the payload has been applied. Grabbing is asynchronous, so the
// counter decides when the last image has landed and quits the process.
QtObject {
  id: root

  property var windows: []
  property string outDir: ""
  property int remaining: 0

  function register(win) {
    var list = windows.slice()
    list.push(win)
    windows = list
  }

  function visibleWindows() {
    var out = []
    for (var i = 0; i < windows.length; i++) {
      if (windows[i] && windows[i].visible) out.push(windows[i])
    }
    return out
  }

  /// Grabs every visible window. `quitAfter` is false when the driver has more to show and will
  /// grab again — the process ends on the last grab, so a driver that wants two frames asks for
  /// the first one without quitting.
  function grabAll(dir, prefix, quitAfter) {
    outDir = dir
    var last = quitAfter === undefined ? true : quitAfter
    var targets = visibleWindows()
    if (targets.length === 0) {
      console.log("harness: FAIL nothing visible to grab")
      Qt.quit()
      return
    }
    remaining = targets.length
    for (var i = 0; i < targets.length; i++) {
      const win = targets[i]
      const name = (prefix ? prefix + "-" : "") + (win.harnessName || ("window-" + i))
      const path = dir + "/" + name + ".png"
      win.contentItem.grabToImage(function(result) {
        if (result && result.saveToFile(path)) console.log("harness: wrote " + path)
        else console.log("harness: FAIL could not write " + path)
        root.remaining -= 1
        if (root.remaining === 0 && last) Qt.quit()
      }, Qt.size(win.width, win.height))
    }
  }
}
