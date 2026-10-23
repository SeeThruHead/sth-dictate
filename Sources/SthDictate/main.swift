import Cocoa

let app = NSApplication.shared
let controller = AppController()
app.setActivationPolicy(.accessory)
controller.launch()
app.run()
