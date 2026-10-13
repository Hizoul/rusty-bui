import QtQuick
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// One card of the app window: the theme's popup surface role, its border spec and
// the theme's corner radius, with children inset by the border plus the surface
// padding — so a hand-written card still follows whatever theme is active.
//
// The default alias is the shape Omarchy's own KeyboardPanel/PopupCard use
// (a `contentItem` alias onto the holder's children), rather than an alias named
// `content` onto `.data`, which qmllint cannot resolve statically.
BorderSurface {
  id: card

  default property alias contentItem: inner.children

  color: Commons.Color.popups.background
  radius: Style.cornerRadius
  borderSpec: Border.surfaceSpec("popups", "border", Commons.Color.popups.border, Math.max(1, Style.space(1)))
  padding: Style.spacing.panelPadding

  implicitHeight: inner.childrenRect.height + contentTopInset + contentBottomInset
  implicitWidth: inner.childrenRect.width + contentLeftInset + contentRightInset

  Item {
    id: inner
    anchors.fill: parent
    anchors.topMargin: card.contentTopInset
    anchors.rightMargin: card.contentRightInset
    anchors.bottomMargin: card.contentBottomInset
    anchors.leftMargin: card.contentLeftInset
  }
}
