import QtQuick

// Stand-in for Quickshell's FileView. The harness has no theme files to read, so every
// view fails to load immediately — which lands colour and style on their declared
// fallbacks, the state a fresh install starts in.
Item {
  id: view

  property url path: ""
  property bool watchChanges: false
  property bool printErrors: false

  signal loaded()
  signal loadFailed()
  signal fileChanged()

  Component.onCompleted: loadFailed()

  function text() {
    return ""
  }

  function reload() {
    loadFailed()
  }
}
