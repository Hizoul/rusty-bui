import QtQuick
import qs.Commons
import qs.Commons as Commons

// Stand-in for qs.Ui.WidgetButton: the bar's text control. The shell gives it the bar's
// font and sizing; here it simply sizes to its label so the pill renders at its natural
// width, which is what the harness wants to look at.
Item {
  id: root

  property var bar: null
  property string text: ""
  property string tooltipText: ""
  property int fontSize: Style.font.body

  signal pressed(int button)

  implicitWidth: label.implicitWidth + Style.space(12)
  implicitHeight: Math.max(label.implicitHeight, fontSize + Style.space(4))

  Text {
    id: label
    anchors.centerIn: parent
    text: root.text
    font.family: Style.font.family
    font.pixelSize: root.fontSize
    color: Commons.Color.foreground
  }
}
