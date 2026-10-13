import QtQuick

// Stand-in for qs.Ui.BarWidget, the bar's plugin entry point. The shell supplies the bar
// handle and the plugin's settings, and offers setting() for defaults; the plugin
// supplies everything else (open/close/opened, panel routing) itself.
Item {
  id: root

  property string moduleName: ""
  property var bar: null
  property var settings: ({})

  function setting(name, fallback) {
    var value = settings ? settings[name] : undefined
    return value === undefined || value === null ? fallback : value
  }
}
