import QtQuick
import qs.Commons

// Stand-in for qs.Ui.BorderSurface: a themed surface with a border spec. The shell draws
// gradient rings and per-side widths; the harness draws a plain bordered, rounded
// rectangle, which is enough to see the layout, the theme colours and the text.
Item {
  id: root

  property color color: "transparent"
  property real radius: 0
  property var borderSpec: null
  property real padding: 0

  readonly property real borderWidth: {
    if (!borderSpec || !borderSpec.widths) return 0
    var widths = borderSpec.widths
    return Math.max(widths.top || 0, widths.right || 0, widths.bottom || 0, widths.left || 0)
  }

  readonly property color borderColor: borderSpec && borderSpec.color ? borderSpec.color : "transparent"

  readonly property real contentTopInset: borderWidth + padding
  readonly property real contentRightInset: borderWidth + padding
  readonly property real contentBottomInset: borderWidth + padding
  readonly property real contentLeftInset: borderWidth + padding

  Rectangle {
    anchors.fill: parent
    color: root.color
    radius: root.radius
    border.width: root.borderWidth
    border.color: root.borderColor
  }
}
