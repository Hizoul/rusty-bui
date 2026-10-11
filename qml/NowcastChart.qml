import QtQuick
import qs.Commons
import qs.Commons as Commons
import "Model.js" as Model

// Bars for the two-hour nowcast: one per radar frame (5 min) or model step
// (15 min), with hour ticks, a now marker and the axis top labelled in mm/h.
// Colours come from the active Omarchy theme via the theme object passed in.
Item {
  id: root

  property var points: []
  property int slotMinutes: 5
  property int hoveredIndex: -1
  property bool clock24: true
  property var theme: ({
    foreground: Commons.Color.foreground,
    accent: Commons.Color.accent,
    urgent: Commons.Color.urgent
  })
  property real plotHeight: Style.space(104)

  signal hovered(int index)

  readonly property real peak: {
    var maximum = 0
    for (var i = 0; i < points.length; i++)
      maximum = Math.max(maximum, Number(points[i].mmPerHour) || 0)
    return maximum
  }
  readonly property real scaleTop: Model.axisTop(peak)
  readonly property var hourMarks: Model.hourMarks(points, clock24)
  readonly property int nowIndex: {
    for (var i = 0; i < points.length; i++)
      if (points[i].isNow) return i
    return 0
  }

  implicitHeight: plotHeight + tickRow.height + Style.space(4)

  function muted(alpha) {
    return Model.withAlpha(root.theme.foreground, alpha)
  }

  // ---- axis: top value and unit, drawn in the gutter left of the plot.
  Item {
    id: axis
    anchors.left: parent.left
    anchors.top: parent.top
    width: Style.space(30)
    height: root.plotHeight

    Text {
      anchors.right: parent.right
      anchors.top: parent.top
      text: root.scaleTop < 1 ? Number(root.scaleTop).toFixed(1) : String(Math.round(root.scaleTop))
      color: root.muted(0.55)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }

    Text {
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      text: "mm/h"
      color: root.muted(0.45)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }
  }

  // ---- plot
  Item {
    id: chart

    property real gap: Style.spaceReal(1)
    // Qualified with the root id: an unqualified `points` resolves through the
    // parent chain, which qmllint flags and which breaks if this item is ever
    // reparented.
    property real barWidth: root.points.length > 0 ? (width - gap * (root.points.length - 1)) / root.points.length : 0

    anchors.left: axis.right
    anchors.leftMargin: Style.space(6)
    anchors.right: parent.right
    anchors.top: parent.top
    height: root.plotHeight

    function indexAt(x) {
      if (barWidth <= 0 || root.points.length === 0) return -1
      const index = Math.floor(x / (barWidth + gap))
      return Math.max(0, Math.min(root.points.length - 1, index))
    }

    Rectangle {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      height: 1
      color: root.muted(0.3)
    }

    Rectangle {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      height: 1
      color: root.muted(0.12)
    }

    // Hour boundaries, so the eye can find the clock times.
    Repeater {
      model: root.hourMarks

      delegate: Rectangle {
        visible: modelData.index > 0
        x: modelData.index * (chart.barWidth + chart.gap)
        width: 1
        height: chart.height
        color: root.muted(0.1)
      }
    }

    Repeater {
      model: root.points

      delegate: Rectangle {
        x: index * (chart.barWidth + chart.gap)
        width: Math.max(1, chart.barWidth)
        anchors.bottom: parent.bottom
        height: {
          var value = Number(modelData.mmPerHour) || 0
          if (value <= 0) return 0
          return Math.max(2, (value / root.scaleTop) * chart.height)
        }
        color: Model.barColor(modelData.class, root.theme)
        border.width: root.hoveredIndex === index ? 1 : 0
        border.color: root.muted(0.8)
      }
    }

    // "Now" marker on the sample the current cycle start falls into.
    Rectangle {
      visible: root.points.length > 0
      x: root.nowIndex * (chart.barWidth + chart.gap) - 1
      width: 1
      height: chart.height
      color: Model.withAlpha(root.theme.accent, 0.7)
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      acceptedButtons: Qt.NoButton
      onPositionChanged: function(mouse) { root.hovered(chart.indexAt(mouse.x)) }
      onExited: root.hovered(-1)
    }
  }

  // ---- tick row: hour labels, plus "now" over the current sample.
  Item {
    id: tickRow
    anchors.left: chart.left
    anchors.right: parent.right
    anchors.top: chart.bottom
    anchors.topMargin: Style.space(3)
    // Every tick shares one font size, and an id inside a Repeater delegate is not
    // reachable from out here — asking a tick for its height is a ReferenceError, so the
    // row measures the font and its own label instead.
    height: Math.max(nowTick.implicitHeight, Style.font.caption)

    Repeater {
      model: root.hourMarks

      delegate: Text {
        id: hourTick
        // "now" is drawn on the sample the cycle starts in, which is usually the same sample
        // as the first hour mark; without this the two labels land on exactly the same x and
        // print "now" and "16:00" on top of each other.
        readonly property bool sharesNow: root.points.length > 0 && root.hoveredIndex < 0
          && modelData.index === root.nowIndex
        x: {
          var position = modelData.index * (chart.barWidth + chart.gap)
          if (sharesNow) position += nowTick.implicitWidth + Style.space(6)
          return Math.min(position, tickRow.width - implicitWidth)
        }
        text: modelData.label
        color: root.muted(0.5)
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
      }
    }

    Text {
      id: nowTick
      visible: root.points.length > 0 && root.hoveredIndex < 0
      x: Math.min(root.nowIndex * (chart.barWidth + chart.gap), tickRow.width - implicitWidth)
      text: "now"
      color: Model.withAlpha(root.theme.accent, 0.9)
      font.family: Style.font.family
      font.pixelSize: Style.font.caption
    }
  }
}
