import QtQuick
import qs.Commons
import qs.Commons as Commons
import "Model.js" as Model

// The two-hour nowcast as one unit: the bar chart, the line under it that says what the pointed-at
// bar is (or, with nothing pointed at, what the next two hours hold), and the status line.
//
// Shared by the bar popout and the app window's nowcast card, so the two cannot drift apart:
// what hovering a bar does is decided here, once. The host supplies the `NowcastView` payload
// and the clock; hover state is this component's own.
Column {
  id: root

  property var view: null
  property bool clock24: true
  property var theme: ({
    foreground: Commons.Color.foreground,
    accent: Commons.Color.accent,
    urgent: Commons.Color.urgent
  })

  property int hoveredIndex: -1

  spacing: Style.space(10)

  /// The sentence under the chart: the pointed-at bar when there is one, else the summary.
  readonly property string detailText: {
    if (!view) return ""
    if (hoveredIndex >= 0 && hoveredIndex < view.points.length) {
      var point = view.points[hoveredIndex]
      return Model.formatClock(point.timeMs, clock24) + " · " + Model.shortIntensity(point.mmPerHour)
        + " mm/h · " + String(point.class).replace("-", " ")
    }
    return Model.summaryLine(view, clock24)
  }

  // A fresh payload can have fewer frames than the one the pointer was resting on.
  onViewChanged: {
    if (!view || hoveredIndex >= view.points.length) hoveredIndex = -1
  }

  function muted(alpha) {
    return Model.withAlpha(root.theme.foreground, alpha)
  }

  NowcastChart {
    width: parent.width
    visible: root.view !== null
    points: root.view ? root.view.points : []
    slotMinutes: root.view ? root.view.slotMinutes : 5
    clock24: root.clock24
    theme: root.theme
    hoveredIndex: root.hoveredIndex
    onHovered: function(index) { root.hoveredIndex = index }
  }

  Text {
    width: parent.width
    text: root.detailText
    wrapMode: Text.WordWrap
    color: root.muted(root.hoveredIndex >= 0 ? 0.95 : 0.8)
    font.family: Style.font.family
    font.pixelSize: Style.font.body
  }

  Text {
    width: parent.width
    visible: root.view !== null
    text: root.view ? Model.statusLine(root.view, root.clock24) : ""
    color: root.muted(0.5)
    font.family: Style.font.family
    font.pixelSize: Style.font.caption
  }
}
