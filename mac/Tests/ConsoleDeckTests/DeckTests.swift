import Foundation
import Testing
@testable import ConsoleDeck

@Test func parsesPresses() {
    #expect(parsePress("D7 PRESSED\r\n") == "D7")
    #expect(parsePress("A0 PRESSED") == "A0")
    #expect(parsePress("D7 released") == nil)
    #expect(parsePress("PONG consoledeck") == nil)
    #expect(parsePress("D13 is LOW at boot (held, shorted to GND, or encoder resting)") == nil)
}

@Test func readsDeckPyConfig() throws {
    let json = #"{"pins": {"D6": 1, "D5": 5}, "buttons": {"1": {"type": "app", "value": "Spotify"}}}"#
    let config = try JSONDecoder().decode(Config.self, from: Data(json.utf8))
    #expect(config.action(forPin: "D6")?.button == 1)
    #expect(config.action(forPin: "D6")?.action == Action(type: "app", value: "Spotify"))
    #expect(config.action(forPin: "D5")?.action.type == "none")
    #expect(config.action(forPin: "D2") == nil)
}

@Test func configRoundTrips() throws {
    let config = Config(pins: ["D6": 1, "A2": 8], buttons: ["1": Action(type: "url", value: "https://x.com")])
    let decoded = try JSONDecoder().decode(Config.self, from: JSONEncoder().encode(config))
    #expect(decoded == config)
    #expect(config.pin(forButton: 8) == "A2")
    #expect(config.pin(forButton: 2) == nil)
}

@Test func frontAppProfileOverridesDefault() {
    let config = Config(
        pins: ["D6": 1, "D7": 2, "D8": 3],
        buttons: ["1": Action(type: "app", value: "Spotify"), "2": Action(type: "app", value: "Finder")],
        profiles: ["com.apple.Safari": Profile(name: "Safari", buttons: [
            "1": Action(type: "url", value: "https://x.com"),
            "2": Action(type: "none"),  // explicitly off in Safari
        ])]
    )
    let safari = "com.apple.Safari"
    #expect(config.action(forPin: "D6", app: safari)?.action.value == "https://x.com")
    #expect(config.action(forPin: "D6", app: safari)?.profile == "Safari")
    #expect(config.action(forPin: "D7", app: safari)?.action.type == "none")
    #expect(config.action(forPin: "D8", app: safari)?.profile == nil)  // not in profile: default
    #expect(config.action(forPin: "D6", app: "com.other")?.action.value == "Spotify")
    #expect(config.action(forPin: "D6")?.profile == nil)
}

@Test func oldConfigWithoutProfilesStillLoads() throws {
    let config = try JSONDecoder().decode(Config.self, from: Data(#"{"pins": {"D6": 1}}"#.utf8))
    #expect(config.profiles.isEmpty && config.buttons.isEmpty && config.pins["D6"] == 1)
}

@Test func ownWindowsKeepPreviousFrontApp() {
    let safari = FrontApp(id: "com.apple.Safari", name: "Safari")
    let own = FrontApp(id: "com.consoledeck.app", name: "ConsoleDeck")
    #expect(frontApp(afterActivating: own, ownID: own.id, previous: safari) == safari)
    #expect(frontApp(afterActivating: safari, ownID: own.id, previous: nil) == safari)
    #expect(frontApp(afterActivating: nil, ownID: own.id, previous: safari) == safari)
}
