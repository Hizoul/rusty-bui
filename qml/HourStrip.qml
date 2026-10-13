import QtQuick
import qs.Commons
import "Model.js" as Model

// A row of hours, scrolled sideways: the clock, the glyph, the temperature and the chance of
// rain, one column each.
//
// Shared by the window's "next hours" card and by the detail under an opened day, so the two
// cannot drift apart — the whole point of opening a day is that its hours look like the hours
// already on screen.
Item {
  id: strip

  property var hours: []
  property color foreground: Commons.Color.foreground
  property color accent: Commons.Color.accent
  property int columnWidth: Style.space(46)
  property int stripHeight: Style.space(96)

  implicitHeight: stripHeight

  Flickable {
    anchors.fill: parent
    contentWidth: row.implicitWidth
    contentHeight: height
    clip: true
    flickableDirection: Flickable.HorizontalFlick
    boundsBehavior: Flickable.StopAtBounds

    Row {
      id: row
      spacing: Style.space(10)

      Repeater {
        model: strip.hours

        delegate: Column {
          width: strip.columnWidth
          spacing: Style.space(3)

          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: modelData.clockLabel
            color: Model.withAlpha(strip.foreground, 0.6)
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
          }

          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: Model.conditionGlyph(modelData.weatherCode, modelData.isDaylight)
            color: strip.accent
            font.family: Style.font.family
            font.pixelSize: Style.font.title
          }

          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: modelData.temperatureLabel || "—"
            color: strip.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }

          Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: modelData.probabilityLabel || ""
            color: Model.withAlpha(strip.foreground, 0.5)
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
          }
        }
      }
    }
  }
}
