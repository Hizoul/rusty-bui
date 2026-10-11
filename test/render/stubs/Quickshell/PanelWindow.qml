import QtQuick
import QtQuick.Window

// Stand-in for Quickshell's layer-shell surface. The harness only needs a real window so
// the QML inside it lays out and can be grabbed; there is no Wayland layer here.
Window {
  id: root

  property string harnessName: "panel-window"

  // Sized to whatever the panel content turned out to be, the way the layer surface
  // would be sized by the shell.
  width: contentItem.childrenRect.width
  height: contentItem.childrenRect.height

  // grabToImage() captures the content item, not the window surface, so the window's own
  // colour has to exist as a rectangle behind the content or every image comes out on
  // transparency.
  Rectangle {
    anchors.fill: parent
    color: root.color
  }

  Component.onCompleted: Harness.register(root)
}
