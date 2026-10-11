import QtQuick
import QtQuick.Window

// Stand-in for Quickshell's FloatingWindow: a real window, so the app window's own implicit
// sizing drives the grab.
//
// QQuickWindow has no implicitWidth/implicitHeight and no minimumSize — Quickshell's
// FloatingWindow adds them (Omarchy's dev-gallery sizes itself with exactly these, the way
// App.qml does), so the stub has to add them too. Leaving them out is not a harmless
// simplification: it reports the app's perfectly good `minimumSize` as an error and sends
// you off fixing working code.
Window {
  id: root

  property string harnessName: "floating-window"
  property real implicitWidth: 640
  property real implicitHeight: 800
  property size minimumSize: Qt.size(0, 0)

  width: implicitWidth
  height: implicitHeight

  // See PanelWindow.qml: grabToImage() does not capture the window's own colour.
  Rectangle {
    anchors.fill: parent
    color: root.color
  }

  Component.onCompleted: Harness.register(root)
}
