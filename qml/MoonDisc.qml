import QtQuick
import QtQuick.Shapes
import qs.Commons
import "Model.js" as Model

// The moon, drawn.
//
// A disc in two arcs — a semicircle for the lit limb and an ellipse for the terminator — rather
// than a glyph, because no nerd font carries all eight phases, and because a crescent should
// look like a crescent rather than like a font that happened to have one. The outline stays
// faintly visible when the moon is nearly new, so the disc does not simply vanish for two days.
//
// The path comes from Model.js, where it is checked against the eight phases.
Item {
  id: moon

  /// 0–1 of the disc the sun is lighting, and which side it is on: right when waxing.
  property real illumination: 0
  property bool waxing: true
  property color color: "#cacccc"
  property real discSize: Style.space(26)

  implicitWidth: discSize
  implicitHeight: discSize

  Rectangle {
    anchors.fill: parent
    radius: width / 2
    // The earthshine: what is left of the disc when the sun is behind it.
    color: Model.withAlpha(moon.color, 0.14)
    border.width: 1
    border.color: Model.withAlpha(moon.color, 0.22)
  }

  Shape {
    anchors.fill: parent
    antialiasing: true
    transform: Translate { x: moon.width / 2; y: moon.height / 2 }

    ShapePath {
      fillColor: moon.color
      strokeColor: "transparent"
      PathSvg {
        path: Model.moonPath(moon.illumination, moon.waxing, Math.max(1, Math.min(moon.width, moon.height) / 2 - 1))
      }
    }
  }
}
