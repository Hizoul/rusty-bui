import QtQuick

// Stand-in for qs.Ui.KeyboardPanel. A layer surface in the shell, a plain item here:
// the popout's content is laid out at the panel's requested content size, which is all
// the harness needs in order to render and grab it.
Item {
  id: root

  property var anchorItem: null
  property var owner: null
  property var bar: null
  property bool open: false
  property bool centerOnBar: false
  property var focusTarget: null
  property real contentWidth: 0
  property real contentHeight: 0

  // The real ones clamp against the screen and the bar; there is no screen here.
  function fittedContentWidth(wanted) {
    return wanted
  }

  function fittedContentHeight(wanted) {
    return wanted
  }

  visible: open
  width: contentWidth
  height: contentHeight
}
