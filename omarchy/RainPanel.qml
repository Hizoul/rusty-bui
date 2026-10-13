import QtQuick
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Detail popout for the rain-nowcast pill: the two-hour chart, what it means in
// words, and where the data came from.
//
// The data is `rustybui bar --json` — the same view contract the standalone app
// consumes. The binary owns fetching, caching and rate limiting, so this panel
// only decides when to ask (on open and once per radar cycle).
//
// Named RainPanel.qml rather than Panel.qml on purpose: a component file named
// after the type it extends makes the type name ambiguous with the imported one.
Panel {
  id: root
  moduleName: "mmb.rainnowcast"
  ipcTarget: "mmb.rainnowcast"

  // Injected by BarWidget.qml.
  property var anchorItem: null
  property var hostWidget: null
  readonly property var barIdentity: hostWidget || root

  property string binaryPath: "rustybui"
  property int pollMinutes: 5
  property bool clock24: true

  // Which parts the pill carries, and which side the icon sits on. Set from the bar widget's
  // own Omarchy settings; the defaults are the pill as it has always looked.
  property bool showIcon: true
  property bool showState: true
  property bool showTemperature: true
  property string iconSide: "left"

  property var view: null
  property string errorText: ""
  property string hintText: ""
  property int hoveredIndex: -1

  readonly property string label: view
    ? Model.pillText(view, view.temperatureLabel, {
        showIcon: root.showIcon,
        showState: root.showState,
        showTemperature: root.showTemperature,
        iconSide: root.iconSide
      })
    : ""

  readonly property string tooltip: {
    if (!view) return errorText !== "" ? errorText : "rain nowcast"
    const lines = []
    lines.push(Model.locationLine(view) + ": " + Model.summaryLine(view, clock24))
    lines.push(Model.providerLine(view) + " · " + Model.statusLine(view, clock24))
    return lines.join("\n")
  }

  readonly property string detailText: {
    if (!view) return ""
    if (hoveredIndex >= 0 && hoveredIndex < view.points.length) {
      const point = view.points[hoveredIndex]
      return Model.formatClock(point.timeMs, clock24) + " · " + Model.shortIntensity(point.mmPerHour) + " mm/h · " + String(point.class).replace("-", " ")
    }
    return Model.summaryLine(view, clock24)
  }

  readonly property string subtitleText: {
    if (!view) return errorText !== "" ? errorText : "loading…"
    return Model.locationLine(view) + " · " + Model.providerLine(view)
  }

  function muted(alpha) {
    return Model.withAlpha(Commons.Color.foreground, alpha)
  }

  function refresh() {
    if (!nowcastProc.running) nowcastProc.running = true
  }

  function open() {
    root.controller.show()
    root.refresh()
  }

  function openFromHotkey() {
    root.controller.show()
    root.refresh()
    // Deferred so the popout handover (which clears the shared flag) has already
    // happened, matching the built-in weather panel.
    Qt.callLater(function() {
      if (root.opened && root.bar && root.bar.setCenterHoverRevealSuppressed)
        root.bar.setCenterHoverRevealSuppressed(true)
    })
  }

  function close() {
    if (root.bar && root.bar.setCenterHoverRevealSuppressed)
      root.bar.setCenterHoverRevealSuppressed(false)
    root.hoveredIndex = -1
    root.controller.hide()
  }

  function toggle() {
    if (root.opened) root.close()
    else root.openFromHotkey()
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.barIdentity, direction)
    return false
  }

  Process {
    id: nowcastProc
    command: [root.binaryPath, "bar", "--json"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyPayload(text)
    }

    onExited: function(exitCode) {
      if (exitCode === 0) return
      root.hintText = root.binaryPath + " exited " + exitCode
    }
  }

  // A payload always wins over an older one; a failure keeps the last good view
  // visible and only records why.
  function applyPayload(raw) {
    var parsed = Model.parseView(raw)
    if (!parsed.ok) {
      root.errorText = parsed.error
      return
    }
    root.view = parsed.view
    root.errorText = ""
    root.hintText = ""
    if (root.hoveredIndex >= parsed.view.points.length) root.hoveredIndex = -1
  }

  // Radar republishes every 5 minutes; polling at that rate costs the binary
  // nothing because it answers from its own cache inside a cycle.
  Timer {
    interval: Math.max(1, root.pollMinutes) * 60 * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    centerOnBar: true
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(420))
    contentHeight: panel.fittedContentHeight(content.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: content
        width: parent.width
        spacing: Style.space(10)

        Item {
          width: parent.width
          height: Math.max(title.implicitHeight, kind.implicitHeight)

          Text {
            id: title
            anchors.left: parent.left
            text: "Rain nowcast"
            color: Commons.Color.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.title
          }

          Text {
            id: kind
            anchors.right: parent.right
            anchors.verticalCenter: title.verticalCenter
            text: root.view ? (root.view.provider.isRadar ? "radar" : "model") : ""
            color: root.muted(0.5)
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
          }
        }

        Text {
          width: parent.width
          text: root.subtitleText
          wrapMode: Text.WordWrap
          color: root.muted(0.6)
          font.family: Style.font.family
          font.pixelSize: Style.font.bodySmall
        }

        NowcastChart {
          width: parent.width
          visible: root.view !== null
          points: root.view ? root.view.points : []
          slotMinutes: root.view ? root.view.slotMinutes : 5
          clock24: root.clock24
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

        Text {
          width: parent.width
          visible: root.errorText !== ""
          text: root.errorText
          wrapMode: Text.WordWrap
          color: Commons.Color.urgent
          font.family: Style.font.family
          font.pixelSize: Style.font.bodySmall
        }

        Text {
          width: parent.width
          visible: root.hintText !== ""
          text: root.hintText + " — build and install it with omarchy/install.sh from the rusty-bui repo"
          wrapMode: Text.WordWrap
          color: Commons.Color.urgent
          font.family: Style.font.family
          font.pixelSize: Style.font.caption
        }
      }
    }
  }
}
