import Foundation

public func herdrPaneID(_ json: Data) -> String? {
    let answer = (try? JSONSerialization.jsonObject(with: json)) as? [String: Any]
    let result = answer?["result"] as? [String: Any]
    let pane = result?["pane"] as? [String: Any]
    return pane?["pane_id"] as? String
}

public func herdrExecutable(home: String, exists: (String) -> Bool) -> String? {
    ["\(home)/.local/bin/herdr", "/opt/homebrew/bin/herdr", "/usr/local/bin/herdr"].first(where: exists)
}
