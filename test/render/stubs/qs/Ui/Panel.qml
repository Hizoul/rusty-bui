import QtQuick
import Quickshell
import qs.Commons
import qs.Commons as Commons

// Stand-in for qs.Ui.Panel for the render harness. Only what the plugin touches is
// here: the module identity, the bar handle, `opened`, and the controller the plugin
// calls to show and hide itself. Sizing follows the content (see PanelWindow.qml).
PanelWindow {
  id: root

  harnessName: "rain-popout"

  property string moduleName: ""
  property string ipcTarget: ""
  property var bar: null
  property bool opened: false

  color: Commons.Color.popups.background

  readonly property alias controller: controller

  QtObject {
    id: controller
    function show() {
      root.opened = true
      root.visible = true
    }
    function hide() {
      root.opened = false
      root.visible = false
    }
    function toggle() {
      if (root.opened) hide()
      else show()
    }
  }
}
