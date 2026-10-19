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
// On a fresh install there is no location yet. The binary says so (`needsLocation`),
// the pill shows a default glyph, and the popout becomes a set-up form: search for a
// place, pick it, and the binary stores it as the default location.
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
  property bool needsLocation: false
  property string errorText: ""
  property string hintText: ""

  // Set-up form state (only used while needsLocation is true).
  property string searchQuery: ""
  property var searchResults: []
  property string searchHint: ""
  property var pendingPlace: null

  // Configured and waiting for the first payload: nothing to show yet, but the pill should
  // still be there so the user knows the plugin is alive. A failed run is not loading.
  readonly property bool loading: !view && !needsLocation && errorText === "" && hintText === ""

  readonly property string label: {
    if (needsLocation) return Model.GLYPH.setup
    if (!view) return loading ? Model.GLYPH.loading : ""
    return Model.pillText(view, view.temperatureLabel, {
      showIcon: root.showIcon,
      showState: root.showState,
      showTemperature: root.showTemperature,
      iconSide: root.iconSide
    })
  }

  readonly property string tooltip: {
    if (needsLocation) return "Rain nowcast: click to set your location"
    if (!view) return errorText !== "" ? errorText : "rain nowcast"
    const lines = []
    lines.push(Model.locationLine(view) + ": " + Model.summaryLine(view, clock24))
    lines.push(Model.providerLine(view) + " · " + Model.statusLine(view, clock24))
    return lines.join("\n")
  }

  readonly property string subtitleText: {
    if (needsLocation) return "No location set yet"
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
    nowcastPanel.hoveredIndex = -1
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
    if (parsed.needsLocation) {
      root.view = null
      root.needsLocation = true
      root.errorText = ""
      root.hintText = ""
      return
    }
    if (!parsed.ok) {
      root.errorText = parsed.error
      return
    }
    root.view = parsed.view
    root.needsLocation = false
    root.errorText = ""
    root.hintText = ""
  }

  // Radar republishes every 5 minutes; polling at that rate costs the binary
  // nothing because it answers from its own cache inside a cycle. Without a location
  // the binary answers instantly, so the set-up state costs nothing either.
  Timer {
    interval: Math.max(1, root.pollMinutes) * 60 * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  // ---- set-up: find a place and store it as the default location

  readonly property var searchCommandLine: {
    var query = String(root.searchQuery || "").replace(/^\s+|\s+$/g, "")
    if (query === "") return []
    return [root.binaryPath, "search", query, "--count", "6", "--json"]
  }

  Process {
    id: searchProc
    command: root.searchCommandLine

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var found = null
        try {
          found = JSON.parse(text)
        } catch (error) {
          root.searchResults = []
          root.searchHint = "search failed"
          return
        }
        root.searchResults = found
        root.searchHint = found.length === 0 ? "no matches" : ""
      }
    }

    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.searchResults = []
        root.searchHint = "search failed (exit " + exitCode + ")"
      }
    }
  }

  function runSearch() {
    if (root.searchCommandLine.length === 0) return
    root.searchHint = "searching…"
    root.searchResults = []
    if (!searchProc.running) searchProc.running = true
  }

  readonly property var setLocationCommandLine: {
    var place = root.pendingPlace
    if (!place) return []
    var command = [
      root.binaryPath, "set-location", place.name,
      "--lat", String(place.latitude),
      "--lon", String(place.longitude)
    ]
    if (place.country_code) command.push("--country", String(place.country_code))
    if (place.timezone) command.push("--timezone", String(place.timezone))
    return command
  }

  Process {
    id: setLocationProc
    command: root.setLocationCommandLine

    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.searchHint = "could not store that location (exit " + exitCode + ")"
        return
      }
      root.searchResults = []
      root.searchQuery = ""
      root.searchHint = ""
      root.pendingPlace = null
      // The location is stored now: ask for the nowcast straight away.
      root.refresh()
    }
  }

  function choosePlace(place) {
    root.pendingPlace = place
    // The command is a binding on the pending place, so it is only complete once the
    // assignment has landed.
    Qt.callLater(function() {
      if (!setLocationProc.running && root.setLocationCommandLine.length > 0)
        setLocationProc.running = true
    })
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
            text: root.needsLocation ? "Set up rain nowcast" : "Rain nowcast"
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

        // Set-up form: shown until a location is stored.
        Column {
          id: setup
          width: parent.width
          spacing: Style.space(8)
          visible: root.needsLocation

          Text {
            width: parent.width
            text: "Search for your town. The nowcast uses the place you pick."
            wrapMode: Text.WordWrap
            color: root.muted(0.6)
            font.family: Style.font.family
            font.pixelSize: Style.font.bodySmall
          }

          Item {
            width: parent.width
            height: Style.space(30)

            Rectangle {
              id: searchBox
              anchors.left: parent.left
              anchors.right: searchButton.left
              anchors.rightMargin: Style.space(8)
              height: parent.height
              radius: Style.cornerRadius
              color: root.muted(0.08)

              TextInput {
                id: searchInput
                anchors.fill: parent
                anchors.leftMargin: Style.space(8)
                anchors.rightMargin: Style.space(8)
                verticalAlignment: TextInput.AlignVCenter
                color: Commons.Color.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                selectByMouse: true
                clip: true
                text: root.searchQuery
                onTextEdited: root.searchQuery = text
                onAccepted: root.runSearch()
              }
            }

            Rectangle {
              id: searchButton
              anchors.right: parent.right
              width: searchLabel.implicitWidth + Style.space(20)
              height: parent.height
              radius: Style.cornerRadius
              color: searchMouse.containsMouse ? root.muted(0.18) : root.muted(0.1)

              Text {
                id: searchLabel
                anchors.centerIn: parent
                text: "Search"
                color: Commons.Color.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.bodySmall
              }

              MouseArea {
                id: searchMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.runSearch()
              }
            }
          }

          Text {
            width: parent.width
            visible: root.searchHint !== ""
            text: root.searchHint
            wrapMode: Text.WordWrap
            color: root.muted(0.5)
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
          }

          Repeater {
            model: root.searchResults

            delegate: Rectangle {
              width: setup.width
              height: Style.space(30)
              radius: Style.cornerRadius
              color: placeMouse.containsMouse ? root.muted(0.14) : "transparent"

              Text {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: Style.space(8)
                anchors.rightMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.name + (modelData.country_code ? "  ·  " + modelData.country_code : "")
                color: Commons.Color.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                elide: Text.ElideRight
              }

              MouseArea {
                id: placeMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.choosePlace(modelData)
              }
            }
          }
        }

        NowcastPanel {
          id: nowcastPanel
          width: parent.width
          visible: root.view !== null
          view: root.view
          clock24: root.clock24
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
