import QtQuick

// Stand-in for Quickshell's StdioCollector. The plugin code reads `text` inside
// onStreamFinished, and Process hands the fixture over through deliver().
Item {
  id: collector

  property bool waitForEnd: true
  property string text: ""

  signal streamFinished(string data)

  function deliver(payload) {
    text = payload
    streamFinished(payload)
  }
}
