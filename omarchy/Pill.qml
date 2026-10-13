import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Ui

// Bar pill for the two-hour rain nowcast.
//
// Shape mirrors the built-in omarchy.weather widget: the shell routes
// open/close/toggle through these functions on the bar-widget root
// (`Bar.findPanelWidget` needs open/close/opened), and the detail popout lives in
// RainPanel.qml, loaded here so it can be injected with the bar, its settings and
// the pill as the popout anchor.
//
// Named Pill.qml rather than BarWidget.qml on purpose: a component file named
// after the type it extends makes the type name ambiguous with the imported one.
BarWidget {
  id: root
  moduleName: "mmb.rainnowcast"

  function injectPanel() {
    var target = panelLoader.item
    if (!target) return
    if ("bar" in target) target.bar = root.bar
    if ("settings" in target) target.settings = root.settings
    if ("anchorItem" in target) target.anchorItem = button
    if ("hostWidget" in target) target.hostWidget = root
    if ("binaryPath" in target) target.binaryPath = String(root.setting("binaryPath", "rustybui"))
    if ("pollMinutes" in target) target.pollMinutes = Number(root.setting("pollMinutes", 5))
    if ("clock24" in target) target.clock24 = root.setting("clock24", true) !== false
    if ("showIcon" in target) target.showIcon = root.setting("showIcon", true) !== false
    if ("showState" in target) target.showState = root.setting("showState", true) !== false
    if ("showTemperature" in target) target.showTemperature = root.setting("showTemperature", true) !== false
    if ("iconSide" in target) target.iconSide = String(root.setting("iconSide", "left")) === "right" ? "right" : "left"
  }

  function refresh() {
    if (panelLoader.item && panelLoader.item.refresh) panelLoader.item.refresh()
  }

  function togglePanel() {
    if (panelLoader.item && panelLoader.item.toggle) panelLoader.item.toggle()
  }

  // Summon/hide routing: the bar identifies this widget as the panel's identity,
  // so `omarchy-shell shell toggle mmb.rainnowcast` and `{ panel = ... }` binds
  // resolve here.
  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false

  function open() {
    if (panelLoader.item && panelLoader.item.openFromHotkey) panelLoader.item.openFromHotkey()
  }

  function close() {
    if (panelLoader.item && panelLoader.item.close) panelLoader.item.close()
  }

  // Forwarded so this widget can stand in for the panel as the bar's popout:
  // Bar.requestPopout prefers closeForPopoutSwitch over close.
  readonly property bool popoutSwitchClosing: panelLoader.item ? panelLoader.item.popoutSwitchClosing === true : false

  function closeForPopoutSwitch() {
    if (panelLoader.item) panelLoader.item.closeForPopoutSwitch()
  }

  // Hidden until the first payload lands, so an unreachable binary shows nothing
  // rather than a reassuring "dry".
  visible: panelLoader.item ? panelLoader.item.label !== "" : false
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("RainPanel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  // The pill is the bar's way into both surfaces: a click on the left opens the whole window, a
  // click on the right the nowcast popout on its own, and the middle button refetches. The bar's
  // WidgetButton reports which button it was, so this is a matter of saying which is which — and
  // right-click is fair game, as the built-in widgets use it for exactly this.
  Process {
    id: windowOpener
    command: ["omarchy-shell", "-q", "shell", "toggle", "mmb.weather", "{}"]
  }

  // A text pill rather than the icon-only slot: the point of this widget is the
  // number — the current rate when it rains, the wait when it does not.
  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: panelLoader.item ? panelLoader.item.label : ""
    tooltipText: panelLoader.item ? panelLoader.item.tooltip : ""
    fontSize: Style.font.body

    onPressed: function(b) {
      if (!root.bar) return
      if (b === Qt.MiddleButton) root.refresh()
      else if (b === Qt.RightButton) root.togglePanel()
      else if (!windowOpener.running) windowOpener.running = true
    }
  }
}
