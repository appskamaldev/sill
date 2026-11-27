// Writes the golden vectors the Windows host's Rust crates are tested against. Every value comes
// from upstream's StreamProtocol public API, so the vectors say what the Mac host and the iOS
// client actually do. Binary values are hex. Deterministic: same source, same files.
//
// Usage: make-vectors <output directory>
import CoreGraphics
import CryptoKit
import Foundation
import StreamProtocol

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "vectors"

func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
func hex(_ b: [UInt8]) -> String { hex(Data(b)) }
func unhex(_ s: String) -> Data {
    var d = Data()
    var i = s.startIndex
    while i < s.endIndex {
        let j = s.index(i, offsetBy: 2)
        d.append(UInt8(s[i..<j], radix: 16)!)
        i = j
    }
    return d
}
func bits64(_ v: Double) -> String { String(format: "%016llx", v.bitPattern) }
func bits32(_ v: Float) -> String { String(format: "%08x", v.bitPattern) }
func sha256(_ s: String) -> Data { Data(SHA256.hash(data: Data(s.utf8))) }

func write(_ name: String, _ obj: [String: Any]) {
    var body: [String: Any] = obj
    body["_source"] = "upstream Sources/StreamProtocol, written by windows/tools/make-vectors.sh; do not edit"
    let data = try! JSONSerialization.data(withJSONObject: body, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
    try! (data + Data("\n".utf8)).write(to: URL(fileURLWithPath: outDir).appendingPathComponent(name))
}

// MARK: - wire.json: header, kinds, caps, binary payloads

var kinds: [[String: Any]] = []
for raw in 0...255 {
    if let k = StreamMessageKind(rawValue: UInt8(raw)) { kinds.append(["raw": raw, "name": String(describing: k)]) }
}

var headers: [[String: Any]] = []
func header(_ name: String, _ kind: StreamMessageKind, _ ts: Double, _ key: Bool, _ payload: Data) {
    let m = StreamMessage(kind: kind, timestamp: ts, isKeyframe: key, payload: payload)
    headers.append(["name": name, "kind": Int(kind.rawValue), "timestampBits": bits64(ts), "keyframe": key,
                    "payload": hex(payload), "serialized": hex(m.serialized())])
}
var ping = Data(); withUnsafeBytes(of: (1764230400.25).bitPattern.bigEndian) { ping.append(contentsOf: $0) }
header("ping", .ping, 1764230400.25, false, ping)
header("keyframe", .frame, 1764230400.123456789, true, unhex("0000000526010af0ff"))
header("delta frame", .frame, 1764230400.5, false, unhex("00000003020100"))
header("parameter sets at time 0", .parameterSets, 0, false, unhex("0401000340010c"))
header("tick, empty", .tick, 1.5, false, Data())
header("hello", .hello, 1764230401, false, Data(#"{"appVersion":"1.0"}"#.utf8))
header("goodbye", .goodbye, 1764230402.75, false, Data(#"{"reason":"quit"}"#.utf8))
header("negative zero time", .keyframeRequest, -0.0, false, Data())
header("next display", .nextDisplay, 0.1, false, Data())
header("tiny fraction", .clipboard, 5e-324, false, Data("{}".utf8))

var parses: [[String: Any]] = []
func parse(_ name: String, _ bytes: Data) {
    var e: [String: Any] = ["name": name, "bytes": hex(bytes)]
    if let h = StreamMessage.parseHeader(bytes) {
        e["ok"] = true
        e["kind"] = Int(h.kind.rawValue)
        e["timestampBits"] = bits64(h.timestamp)
        e["keyframe"] = h.isKeyframe
        e["payloadLength"] = h.payloadLength
    } else {
        e["ok"] = false
    }
    parses.append(e)
}
for h in headers { parse("header of \(h["name"]!)", unhex(String((h["serialized"] as! String).prefix(28)))) }
func raw(_ kind: UInt8, _ tsBits: UInt64, _ key: UInt8, _ len: UInt32) -> Data {
    var d = Data([kind])
    withUnsafeBytes(of: tsBits.bigEndian) { d.append(contentsOf: $0) }
    d.append(key)
    withUnsafeBytes(of: len.bigEndian) { d.append(contentsOf: $0) }
    return d
}
parse("unknown kind 200", raw(200, (1764230400.0).bitPattern, 1, 5))
parse("reserved kind 29 reads as unknown", raw(29, (1.5).bitPattern, 0, 10))
parse("kind 255", raw(255, 0, 0, 0))
parse("keyframe byte 2 is not a keyframe", raw(1, 0, 2, 10))
parse("keyframe byte 1", raw(1, 0, 1, 10))
parse("length 0xffffffff", raw(1, 0, 0, 0xFFFF_FFFF))
parse("NaN timestamp bits kept", raw(11, 0x7FF8_0000_0000_0001, 0, 8))
parse("13 bytes", unhex("01000000000000000000000000"))
parse("empty", Data())
parse("trailing bytes after the header are ignored", raw(11, (1.0).bitPattern, 0, 8) + unhex("aabb"))

func paramSets(_ sets: [Data], nalLen: Int = 4) -> [String: Any] {
    ["nalLen": nalLen, "sets": sets.map(hex), "encoded": hex(ParameterSets(nalUnitHeaderLength: nalLen, sets: sets).encoded())]
}
let psEncode: [[String: Any]] = [
    paramSets([unhex("40010c01ffff"), unhex("420101016000"), unhex("4401c172b4")]),
    paramSets([]),
    paramSets([Data()], nalLen: 2),
]
var psDecode: [[String: Any]] = []
for (name, h) in [("valid", "040300064001 0c01ffff0006420101016000 00054401c172b4"), ("one byte", "04"),
                  ("zero sets", "0400"), ("truncated length", "040100"), ("truncated body", "0401000340"),
                  ("trailing bytes ignored", "04010001aa ffff"), ("count 2, one set", "04020001aa"),
                  ("empty set", "0401 0000")] {
    let d = unhex(h.replacingOccurrences(of: " ", with: ""))
    var e: [String: Any] = ["name": name, "bytes": hex(d)]
    if let p = ParameterSets(encoded: d) { e["ok"] = true; e["nalLen"] = p.nalUnitHeaderLength; e["sets"] = p.sets.map(hex) } else { e["ok"] = false }
    psDecode.append(e)
}

let thumbEncode: [[String: Any]] = [
    ["windowID": 0xDEADBEEF, "jpeg": "ffd8ffd9", "encoded": hex(ImageBlob.encodeThumbnail(windowID: 0xDEADBEEF, jpeg: unhex("ffd8ffd9")))],
    ["windowID": 1, "jpeg": "", "encoded": hex(ImageBlob.encodeThumbnail(windowID: 1, jpeg: Data()))],
]
var thumbDecode: [[String: Any]] = []
for h in ["00000001", "00000001ff", "deadbeefffd8ffd9", "", "000000"] {
    var e: [String: Any] = ["bytes": h]
    if let t = ImageBlob.decodeThumbnail(unhex(h)) { e["ok"] = true; e["windowID"] = Int(t.windowID); e["jpeg"] = hex(t.jpeg) } else { e["ok"] = false }
    thumbDecode.append(e)
}
let iconEncode: [[String: Any]] = [
    ["bundleID": "com.apple.Safari", "png": "89504e47", "encoded": hex(ImageBlob.encodeIcon(bundleID: "com.apple.Safari", png: unhex("89504e47")))],
    ["bundleID": "", "png": "89", "encoded": hex(ImageBlob.encodeIcon(bundleID: "", png: unhex("89")))],
    ["bundleID": "é.app", "png": "", "encoded": hex(ImageBlob.encodeIcon(bundleID: "é.app", png: Data()))],
]
var iconDecode: [[String: Any]] = []
for h in ["0000", "000041", "0003616263", "000361626389", "0001ff00", "00", "", "0002c3a989", "0010" + hex(Data("com.apple.Safari".utf8)) + "89504e47"] {
    var e: [String: Any] = ["bytes": h]
    if let t = ImageBlob.decodeIcon(unhex(h)) { e["ok"] = true; e["bundleID"] = t.bundleID; e["png"] = hex(t.png) } else { e["ok"] = false }
    iconDecode.append(e)
}
func cursor(_ hx: Double, _ hy: Double, _ w: Double, _ h: Double, _ png: String) -> [String: Any] {
    ["hotX": hx, "hotY": hy, "width": w, "height": h, "png": png,
     "encoded": hex(CursorShapeBlob.encode(hotspot: CGPoint(x: hx, y: hy), size: CGSize(width: w, height: h), png: unhex(png)))]
}
let cursorEncode = [cursor(3.5, 4, 16, 24, "89504e47"), cursor(0.1, 1.0 / 3.0, 32, 32, "00"), cursor(0, 0, 0, 0, "")]
var cursorDecode: [[String: Any]] = []
for h in [String(repeating: "00", count: 16), String(repeating: "00", count: 16) + "89",
          "40600000408000004180000041c0000089504e47", "3dcccccd3eaaaaab420000004200000000", "00"] {
    var e: [String: Any] = ["bytes": h]
    if let c = CursorShapeBlob.decode(unhex(h)) {
        e["ok"] = true
        e["hotXBits"] = bits32(Float(c.hotspot.x)); e["hotYBits"] = bits32(Float(c.hotspot.y))
        e["widthBits"] = bits32(Float(c.size.width)); e["heightBits"] = bits32(Float(c.size.height))
        e["png"] = hex(c.png)
    } else { e["ok"] = false }
    cursorDecode.append(e)
}

write("wire.json", [
    "caps": ["headerLength": StreamMessage.headerLength, "maxClientPayload": StreamMessage.maxClientPayload,
             "maxPairingPayload": StreamMessage.maxPairingPayload, "maxFramePayload": StreamMessage.maxFramePayload,
             "maxOtherHostPayload": StreamMessage.maxOtherHostPayload],
    "kinds": kinds,
    "headers": headers,
    "parseHeader": parses,
    "parameterSets": ["encode": psEncode, "decode": psDecode],
    "thumbnail": ["encode": thumbEncode, "decode": thumbDecode],
    "appIcon": ["encode": iconEncode, "decode": iconDecode],
    "cursorShape": ["encode": cursorEncode, "decode": cursorDecode],
    "protocol": ["current": SillProtocol.current],
])

// MARK: - json.json: every JSON payload, encoded and decoded

let plain = JSONEncoder()
let sorted: JSONEncoder = { let e = JSONEncoder(); e.outputFormatting = [.sortedKeys]; return e }()
var encodes: [[String: Any]] = []
var decodes: [[String: Any]] = []

func enc<T: Encodable>(_ type: String, _ name: String, _ v: T) {
    encodes.append(["type": type, "name": name,
                    "json": String(decoding: try! plain.encode(v), as: UTF8.self),
                    "sorted": String(decoding: try! sorted.encode(v), as: UTF8.self)])
}
func dec<T: Codable>(_ t: T.Type, _ type: String, _ inputs: [String]) {
    for input in inputs {
        var e: [String: Any] = ["type": type, "input": input]
        do {
            let v = try JSONDecoder().decode(t, from: Data(input.utf8))
            e["ok"] = true
            e["sorted"] = String(decoding: try sorted.encode(v), as: UTF8.self)
        } catch {
            e["ok"] = false
        }
        decodes.append(e)
    }
}

enc("StreamSource", "none", StreamSource.none)
enc("StreamSource", "desktop", StreamSource.desktop)
enc("StreamSource", "window", StreamSource.window(1234))
enc("StreamSource", "window max", StreamSource.window(UInt32.max))
dec(StreamSource.self, "StreamSource", [
    #"{"none":{}}"#, #"{"desktop":{}}"#, #"{"window":{"_0":42}}"#, #"{"window":{"_0":42,"extra":true}}"#,
    #"{"window":{"_0":-1}}"#, #"{"window":{"_0":4294967296}}"#, #"{"window":{"_0":42.0}}"#, #"{"window":{"_0":42.5}}"#,
    #"{"window":{}}"#, #"{"window":{"_0":"42"}}"#, #"{}"#, #"{"none":{},"desktop":{}}"#, #"{"desktop":{},"x":1}"#,
    #"{"bogus":{}}"#, #""desktop""#, #"{"none":5}"#, #"{"none":null}"#, #"{"none":[]}"#, #"{"desktop":{"_0":1}}"#,
    #"{"window":{"_0":4294967295}}"#, #"{"window":{"_0":1e3}}"#, #"{"window":{"_0":null}}"#, "null", "[]",
])

enc("InputEvent", "pointer leftDown", InputEvent.pointer(.leftDown, x: 0.25, y: 0.5))
enc("InputEvent", "pointer move at corners", InputEvent.pointer(.move, x: 0, y: 1))
enc("InputEvent", "pointer rightUp", InputEvent.pointer(.rightUp, x: 0.123456789, y: 0.987654321))
enc("InputEvent", "pointer leftUp", InputEvent.pointer(.leftUp, x: 0.1, y: 0.2))
enc("InputEvent", "pointer rightDown", InputEvent.pointer(.rightDown, x: 1e-7, y: 0.3))
enc("InputEvent", "scroll", InputEvent.scroll(x: 0.5, y: 0.5, dx: 0, dy: 0.03))
enc("InputEvent", "scroll negative", InputEvent.scroll(x: 0.5, y: 0.5, dx: -0.015625, dy: -1.5))
enc("InputEvent", "text", InputEvent.text("hé"))
enc("InputEvent", "text controls", InputEvent.text("\n\t\u{8}\"\\/"))
enc("InputEvent", "text emoji", InputEvent.text("👍🏽"))
for p in [ScrollPhase.began, .ended, .momentumBegan, .momentumEnded] {
    enc("InputEvent", "scrollGesture \(p.rawValue)", InputEvent.scrollGesture(p, x: 0.5, y: 0.75))
}
enc("InputEvent", "key", InputEvent.key(hidUsage: 4, down: true, modifiers: 1 << 20))
enc("InputEvent", "key up, all modifiers", InputEvent.key(hidUsage: 0xE7, down: false, modifiers: (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20)))
enc("InputEvent", "key max", InputEvent.key(hidUsage: UInt16.max, down: false, modifiers: UInt64.max))
dec(InputEvent.self, "InputEvent", [
    #"{"pointer":{"_0":"leftDown","x":0.25,"y":0.5}}"#, #"{"pointer":{"_0":"move","x":0,"y":1}}"#,
    #"{"pointer":{"_0":"drag","x":0,"y":0}}"#, #"{"pointer":{"x":0,"y":0}}"#, #"{"pointer":{"_0":"move","x":0}}"#,
    #"{"scroll":{"x":0.5,"y":0.5,"dx":0,"dy":1}}"#, #"{"scroll":{"x":0.5,"y":0.5,"dy":1}}"#,
    #"{"text":{"_0":""}}"#, #"{"text":{"_0":"a\nb"}}"#, #"{"text":{}}"#, #"{"text":"a"}"#,
    #"{"scrollGesture":{"_0":"began","x":0.5,"y":0.5}}"#, #"{"scrollGesture":{"_0":"cancelled","x":0.5,"y":0.5}}"#,
    #"{"key":{"hidUsage":4,"down":true,"modifiers":1048576}}"#, #"{"key":{"hidUsage":4,"down":true}}"#,
    #"{"key":{"hidUsage":65536,"down":true,"modifiers":0}}"#, #"{"key":{"hidUsage":4,"down":1,"modifiers":0}}"#,
    #"{"key":{"hidUsage":4,"down":true,"modifiers":18446744073709551615}}"#, #"{"key":{"hidUsage":4,"down":true,"modifiers":-1}}"#,
    #"{"key":{"hidUsage":4,"down":true,"modifiers":0,"repeat":true}}"#, #"{"tap":{}}"#,
    #"{"pointer":{"_0":"leftDown","x":0.25,"y":0.5},"text":{"_0":"x"}}"#, #"{"pointer":{"_0":"leftDown","x":"0.25","y":0.5}}"#,
])

enc("Viewport", "panel only", Viewport(width: 390, height: 844, scale: nil))
enc("Viewport", "all fields", Viewport(width: 1366.5, height: 1024, scale: 1.25, localCursor: true, fps: 120))
dec(Viewport.self, "Viewport", [
    #"{"width":390,"height":844}"#, #"{"width":390,"height":844,"scale":null}"#, #"{"width":390}"#,
    #"{"width":390,"height":844,"scale":1.5,"localCursor":false,"fps":60,"future":[1,2]}"#,
    #"{"width":390,"height":844,"fps":59.5}"#, #"{"width":1e300,"height":-5}"#, #"{"width":"390","height":844}"#,
])

enc("ClientStats", "first four", ClientStats(fps: 60, frameAgeMs: 18, rttMs: 4, device: "iPad (iPad14,1)"))
enc("ClientStats", "all", ClientStats(fps: 120, frameAgeMs: -1, rttMs: -1, device: "iPhone", frameAgeMaxMs: 40, rttMaxMs: 9, audioBehindMs: 30, audioLate: 2))
dec(ClientStats.self, "ClientStats", [
    #"{"fps":60,"frameAgeMs":18,"rttMs":4,"device":"iPad"}"#, #"{"fps":60,"frameAgeMs":18,"rttMs":4}"#,
    #"{"fps":60,"frameAgeMs":18,"rttMs":4,"device":"iPad","frameAgeMaxMs":null,"audioLate":3}"#,
])

enc("Hello", "empty", Hello())
enc("Hello", "full", Hello(appVersion: "1.0", build: "42", protocol: 1, device: "iPad (iPad14,1)", audio: ["aac-eld"], offConsoleView: 1))
dec(Hello.self, "Hello", [
    #"{}"#, #"{"appVersion":"1.0","build":"42","protocol":1,"device":"iPad","audio":["aac-eld"],"offConsoleView":1}"#,
    #"{"protocol":"1"}"#, #"{"appVersion":1}"#, #"{"audio":"aac-eld"}"#, #"{"device":"x","later":{"a":1}}"#, #"[]"#,
])

let win = WindowInfo(id: 77, title: "Notes — “Plan”", appName: "Notes", bundleID: "com.apple.Notes", width: 800, height: 600)
enc("WindowInfo", "window", win)
enc("AppInfo", "app", AppInfo(name: "Safari", bundleID: "com.apple.Safari"))
enc("WindowList", "minimal", WindowList(macName: "PC", windows: [], active: .none))
var fullList = WindowList(macName: "Kamal's PC", windows: [win], active: .window(77), launchID: "7F3A", hostVersion: "0.1.0",
                          protocol: 1, gestures: 1, clipboard: 1)
fullList.keyRepeat = 1
fullList.desktopDisplays = 2; fullList.desktopDisplay = 1; fullList.desktopDisplayNames = ["DELL U2723QE", ""]
fullList.locked = true; fullList.offConsole = true; fullList.offConsoleView = true
enc("WindowList", "every optional", fullList)
enc("WindowList", "desktop, protocol only", WindowList(macName: "PC", windows: [], active: .desktop, protocol: 1))
dec(WindowList.self, "WindowList", [
    #"{"macName":"PC","windows":[],"active":{"none":{}}}"#, #"{"macName":"PC","windows":[]}"#,
    #"{"macName":"PC","windows":[{"id":1,"title":"t","appName":"a","bundleID":"b","width":1,"height":2}],"active":{"window":{"_0":1}},"protocol":1,"later":true}"#,
    #"{"macName":"PC","windows":[{"id":1,"title":"t","appName":"a","bundleID":"b","width":1}],"active":{"desktop":{}}}"#,
    #"{"macName":"PC","windows":[],"active":{"desktop":{}},"clipboard":true}"#,
    #"{"macName":"PC","windows":[[1,"t","a","b",1,2]],"active":{"desktop":{}}}"#, #"["PC",[],{"none":{}}]"#,
])
enc("LaunchApp", "launch", LaunchApp(bundleID: "com.apple.Safari"))
enc("WindowCommand", "close", WindowCommand(id: 77, action: .close))
enc("WindowCommand", "fullScreen", WindowCommand(id: 1, action: .fullScreen))
enc("WindowCommand", "minimize", WindowCommand(id: 2, action: .minimize))
dec(WindowCommand.self, "WindowCommand", [#"{"id":1,"action":"close"}"#, #"{"id":1,"action":"maximize"}"#, #"{"action":"close"}"#])
dec(LaunchApp.self, "LaunchApp", [#"{"bundleID":"x"}"#, #"{}"#])

let fixed = StreamSettings(maxFPS: 60, bitrate: 15_000_000, captureScale: 1, prioritizeSpeed: true, virtualDisplay: false,
                           directWireless: nil, sendAudio: nil)
enc("StreamSettings", "required only", fixed)
enc("HostSettingsState", "the Windows MVP's", HostSettingsState(settings: fixed, persistent: false, virtualDisplayAvailable: false, softwareEncoder: false))
enc("HostSettingsState", "with stream and answer", HostSettingsState(
    settings: StreamSettings(maxFPS: 120, bitrate: 40_000_000, captureScale: 2, prioritizeSpeed: false, virtualDisplay: true,
                             directWireless: true, sendAudio: false),
    persistent: true, virtualDisplayAvailable: true, virtualDisplayNote: "note", softwareEncoder: true,
    stream: RunningStream(width: 2732, height: 2048, fps: 120, mbps: 80, onVirtualDisplay: true), answering: 7,
    away: AwayQuality(homeBitrate: 15_000_000, homeCaptureScale: 2, awayBitrate: 4_000_000, awayCaptureScale: 1, thisConnectionAway: false, awayRunning: false),
    link: LinkReport(state: LinkReport.behind, withheldPerSecond: 12, bitrate: 15_000_000, carriedKbps: 9000, suggestedBitrate: 8_000_000, suggestedCaptureScale: 1),
    audioNote: "a", sessionPaired: true))
dec(HostSettingsState.self, "HostSettingsState", [
    #"{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true,"virtualDisplay":false},"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false}"#,
    #"{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true,"virtualDisplay":false},"persistent":false,"virtualDisplayAvailable":false}"#,
    #"{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true},"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false}"#,
    #"{"settings":[60,15000000,1,true,false],"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false}"#,
    #"{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true,"virtualDisplay":false},"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false,"stream":[1,2,3,4,true]}"#,
    #"{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true,"virtualDisplay":false,"directWireless":null},"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false,"link":{"state":"weird","withheldPerSecond":0,"bitrate":1}}"#,
])
enc("HostSettingsChange", "empty", HostSettingsChange())
enc("HostSettingsChange", "every field", HostSettingsChange(token: 3, maxFPS: 60, bitrate: 8_000_000, captureScale: 2, prioritizeSpeed: false,
                                                            virtualDisplay: true, directWireless: false, sendAudio: true))
dec(HostSettingsChange.self, "HostSettingsChange", [#"{}"#, #"{"token":3,"maxFPS":60}"#, #"{"token":"3"}"#, #"{"captureScale":2,"unknown":1}"#])

enc("Goodbye", "quit", Goodbye(reason: Goodbye.quit))
enc("Goodbye", "update", Goodbye(reason: Goodbye.update, message: "Update Sill on your iPad to keep using PC. It needs version 1.2 or later.",
                                 minimumVersion: "1.2", reconnect: false))
enc("Goodbye", "restarting", Goodbye(reason: Goodbye.restarting, message: "Restarting", reconnect: true))
dec(Goodbye.self, "Goodbye", [#"{"reason":"quit"}"#, #"{}"#, #"{"reason":"later","message":"m","reconnect":true,"x":0}"#])

enc("PairRequest", "code", PairRequest(method: PairRequest.code, proof: "XAX-uZPsq_Ax8fvI_sV7Ex0a-ROi19rI-FW-ykPh9TE", name: "iPad", model: "iPad14,1"))
enc("PairRequest", "ask on the cable", PairRequest(method: PairRequest.ask, proof: "", name: "iPhone", model: nil, cable: true))
dec(PairRequest.self, "PairRequest", [
    #"{"v":1,"method":"code","proof":"abc","name":"iPad"}"#, #"{"v":1,"method":"code","proof":"abc"}"#,
    #"{"method":"code","proof":"abc","name":"iPad"}"#, #"{"v":2,"method":"qr","proof":"","name":"","model":"m","cable":false,"later":1}"#,
])
enc("PairResult", "ok", PairResult(ok: true, proof: "6BmGWC9XT8J8OY8phhq9bASgoSnoWpRxBeqZtvRrWmk", macID: "A1B2C3D4E5F6G7H8", name: "PC",
                                   recognitionKey: Base64URL.encode(Data(repeating: 0x11, count: 32))))
enc("PairResult", "wrong code", PairResult(ok: false, reason: PairResult.code, triesLeft: 4))
enc("PairResult", "busy", PairResult(ok: false, reason: PairResult.busy, retryAfter: 1.5))
enc("PairResult", "busy whole seconds", PairResult(ok: false, reason: PairResult.busy, retryAfter: 5))
enc("PairResult", "cable", PairResult(ok: true, macID: "A1B2C3D4E5F6G7H8", name: "PC", recognitionKey: "x", method: PairResult.cable))
enc("PairResult", "unknown reason with message", PairResult(ok: false, reason: "later", message: "Words"))
dec(PairResult.self, "PairResult", [#"{"ok":false,"reason":"code","triesLeft":4}"#, #"{"reason":"code"}"#, #"{"ok":true,"retryAfter":2}"#])

let point = try! P256.Signing.PrivateKey(rawRepresentation: Data(repeating: 0x11, count: 32))
let pub = point.publicKey.x963Representation
let fp = SPKI.fingerprint(point: pub)
let macInfo = MacInfo(macID: MacID.make(fingerprint: fp), name: "PC", issuedAt: 1764230400.5, remoteAccess: true, remotePort: 7455,
                      internet: false, addresses: [MacAddress(host: "100.101.102.103", kind: MacAddress.vpn, via: "Tailscale"),
                                                   MacAddress(host: "home.example.net", port: 17455, kind: MacAddress.internet, via: "Address name")])
enc("MacInfo", "info", macInfo)
enc("MacAddress", "with port", MacAddress(host: "fd7a:115c:a1e0::1234", port: 7456, kind: MacAddress.vpn, via: "Tailscale"))
dec(MacInfo.self, "MacInfo", [
    #"{"v":1,"macID":"X","name":"n","issuedAt":1,"remoteAccess":false,"remotePort":7455,"internet":false,"addresses":[]}"#,
    #"{"v":1,"macID":"X","name":"n","issuedAt":1,"remoteAccess":false,"remotePort":7455,"internet":false}"#,
])

// Kind 18's signature: ECDSA is randomized, so one signature made once is kept here and checked
// to verify with upstream's own `verified()`; the info bytes are fixed text for the same reason.
let infoBytes = Data(#"{"addresses":[{"host":"100.101.102.103","kind":"vpn","via":"Tailscale"}],"internet":false,"issuedAt":1764230400.5,"macID":"MACID","name":"PC","remoteAccess":true,"remotePort":7455,"v":1}"#
    .replacingOccurrences(of: "MACID", with: MacID.make(fingerprint: fp)).utf8)
let pinnedSig = "MEQCIGjhTJbTmVGgQKdXzMrsxDPVpMm/G5pwCg0Mpkw61iaEAiAWZE8Hub62fS2YxMeHdt2SUi9t+5qUTsGm5WQaeyIAJA=="
var signed = SignedMacInfo(info: infoBytes.base64EncodedString(), key: pub.base64EncodedString(), sig: pinnedSig)
if signed.verified() == nil {
    let fresh = try! point.signature(for: infoBytes).derRepresentation.base64EncodedString()
    FileHandle.standardError.write(Data("make-vectors: the pinned kind 18 signature no longer verifies; set pinnedSig to \(fresh)\n".utf8))
    exit(1)
}
enc("SignedMacInfo", "signed", signed)
// The same bytes signed by sill-pairing's `sign` (p256, RFC 6979, so fixed): upstream must take it.
let rustSig = "MEYCIQCtPd/8cKCP+m6r4qj7Og2HUW1d89aBv2zAJA1Tudc8mAIhANckbI3PAE0hVf99HwoWJUqRlI446DJQ4M4EVAlasLPw"
var rustSigned = signed; rustSigned.sig = rustSig
var tampered = signed; tampered.info = Data(infoBytes.map { $0 == UInt8(ascii: "5") ? UInt8(ascii: "6") : $0 }).base64EncodedString()
var wrongKey = signed; wrongKey.key = try! P256.Signing.PrivateKey(rawRepresentation: Data(repeating: 0x22, count: 32)).publicKey.x963Representation.base64EncodedString()
let signedVectors: [String: Any] = [
    "privateKey": hex(point.rawRepresentation), "publicKey": hex(pub), "infoBytes": hex(infoBytes),
    "valid": ["info": signed.info, "key": signed.key, "sig": signed.sig, "verifies": signed.verified() != nil,
              "fingerprint": hex(signed.verified()!.fingerprint)],
    "tampered": ["info": tampered.info, "key": tampered.key, "sig": tampered.sig, "verifies": tampered.verified() != nil],
    "rustSigned": ["info": rustSigned.info, "key": rustSigned.key, "sig": rustSigned.sig, "verifies": rustSigned.verified() != nil],
    "wrongKey": ["info": wrongKey.info, "key": wrongKey.key, "sig": wrongKey.sig, "verifies": wrongKey.verified() != nil],
]
dec(SignedMacInfo.self, "SignedMacInfo", [#"{"info":"e30=","key":"","sig":""}"#, #"{"info":"e30=","key":""}"#])

enc("TrackpadGesture", "swipe up, three fingers", TrackpadGesture(gesture: TrackpadGesture.swipeUp, fingers: 3))
enc("TrackpadGesture", "pinch", TrackpadGesture(gesture: TrackpadGesture.pinch))
dec(TrackpadGesture.self, "TrackpadGesture", [#"{"gesture":"swipeUp","fingers":3}"#, #"{"gesture":"pinch"}"#, #"{}"#, #"{"gesture":"spin","fingers":5}"#])
enc("Clipboard", "empty (subscribe)", Clipboard())
enc("Clipboard", "text", Clipboard(text: "copied \u{1F4CB}"))
enc("Clipboard", "image", Clipboard(image: unhex("89504e470d0a1a0a")))
enc("Clipboard", "stop", Clipboard(stop: true))
dec(Clipboard.self, "Clipboard", [#"{}"#, #"{"text":"hi"}"#, #"{"image":"iVBORw0KGgo="}"#, #"{"image":"iVBORw0KGgo"}"#,
                                   #"{"image":"not base64!"}"#, #"{"image":""}"#, #"{"stop":true,"text":null}"#, #"{"image":"iVBO\/w=="}"#,
                                   #"{"image":"Zh=="}"#, #"{"image":"Zg"}"#, #"{"image":" Zg=="}"#, #"{"image":"Zg==\n"}"#, #"{"image":"Zm8="}"#, #"{"image":"Zm9v===="}"#, #"{"image":"Z-8="}"#])
enc("MacPointer", "empty", MacPointer())
enc("MacPointer", "inside", MacPointer(x: 0.5, y: 0.25, inside: true, seen: 3))
enc("MacPointer", "outside", MacPointer(inside: false, seen: 4))
dec(MacPointer.self, "MacPointer", [#"{}"#, #"{"x":0.5,"y":0.25,"inside":true,"seen":3}"#, #"{"seen":1.5}"#])

write("json.json", ["encode": encodes, "decode": decodes, "signedMacInfo": signedVectors])

// MARK: - pairing.json

var damm: [[String: Any]] = []
for digits in [[], [5, 7, 2], [4, 8, 2, 9, 1, 3, 5, 5, 7, 2, 0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], [9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9],
               [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1]] {
    damm.append(["digits": digits, "check": PairingCode.checkDigit(digits)])
}
var codeChecks: [[String: Any]] = []
for input in ["482913557208", "4829 1355 7208", "4829-1355-7208", "4829\u{00A0}1355\u{00A0}7208", "482913557209", "48291355720",
              "4829135572080", "48291355720a", "", "４８２９１３５５７２０８", "4829\t1355 7208", "000000000000", "  482913557208  ",
              "١٢٣٤٥٦٧٨٩٠١٢"] {
    switch PairingCode.check(input) {
    case .success(let c): codeChecks.append(["input": input, "ok": c])
    case .failure(let p): codeChecks.append(["input": input, "problem": p == .length ? "length" : "typo"])
    }
}
let grouped = ["482913557208", "12345", "", "1234", "123456789"].map { ["code": $0, "grouped": PairingCode.grouped($0)] }

let fpMac = sha256("mac"), fpDev = sha256("device")
let secret = Data(repeating: 0x22, count: 16)
let codeK = PairingProof.codeKey(code: "482913557208", macFingerprint: fpMac)!.withUnsafeBytes { Data($0) }
let qrK = Data(secret)
func proofs(_ name: String, _ key: Data) -> [String: Any] {
    let k = SymmetricKey(data: key)
    let d = PairingProof.deviceProof(key: k, deviceFingerprint: fpDev, macFingerprint: fpMac)
    let m = PairingProof.macProof(key: k, macFingerprint: fpMac, deviceFingerprint: fpDev)
    return ["name": name, "key": hex(key), "fpDevice": hex(fpDev), "fpMac": hex(fpMac), "deviceProof": hex(d), "macProof": hex(m),
            "deviceProofB64": Base64URL.encode(d), "macProofB64": Base64URL.encode(m),
            "deviceValid": PairingProof.isValidDeviceProof(d, key: k, deviceFingerprint: fpDev, macFingerprint: fpMac),
            "swappedValid": PairingProof.isValidDeviceProof(d, key: k, deviceFingerprint: fpMac, macFingerprint: fpDev),
            "macAsDeviceValid": PairingProof.isValidDeviceProof(m, key: k, deviceFingerprint: fpDev, macFingerprint: fpMac),
            "truncatedValid": PairingProof.isValidDeviceProof(d.prefix(31), key: k, deviceFingerprint: fpDev, macFingerprint: fpMac),
            "macValid": PairingProof.isValidMacProof(m, key: k, macFingerprint: fpMac, deviceFingerprint: fpDev)]
}
let codeKeys: [[String: Any]] = [
    ["code": "482913557208", "fpMac": hex(fpMac), "key": hex(codeK)],
    ["code": "000000000000", "fpMac": hex(fpDev), "key": hex(PairingProof.codeKey(code: "000000000000", macFingerprint: fpDev)!.withUnsafeBytes { Data($0) })],
    ["code": "48291355720", "fpMac": hex(fpMac), "key": NSNull()],
    ["code": "48291355720a", "fpMac": hex(fpMac), "key": NSNull()],
    ["code": "４８２９１３５５７２０８", "fpMac": hex(fpMac), "key": NSNull()],
]

let rk = Data(repeating: 0x11, count: 32)
let tag = RecognitionTag.make(recognitionKey: rk, nonce: unhex("a1a2a3a4a5a6"))!
let tag2 = RecognitionTag.make(recognitionKey: Data(repeating: 0x33, count: 32), nonce: unhex("000000000000"))!
var tagMatches: [[String: Any]] = []
for (t, key) in [(tag, rk), (tag, Data(repeating: 0x12, count: 32)), (tag2, Data(repeating: 0x33, count: 32)), ("short", rk),
                 (tag + "==", rk), (String(tag.dropLast()) + "n", rk), ("", rk), (tag, Data())] {
    tagMatches.append(["tag": t, "key": hex(key), "matches": RecognitionTag.matches(t, recognitionKey: key)])
}

let b64Encode = ["", "66", "666f", "666f6f", "fbff", "fbffbf", "00", hex(fp)].map { ["bytes": $0, "text": Base64URL.encode(unhex($0))] }
var b64Decode: [[String: Any]] = []
for t in ["", "-_8", "+/8", "Zg==", "Zg", "Zh", "Z", "Zm9v", " Zg", "Zm9v\n", "Zm9vYg", "Zm9vYh", "Zm9vYmE", "Zm9vYmF", "é"] {
    b64Decode.append(["text": t, "bytes": Base64URL.decode(t).map(hex) ?? NSNull()])
}

let example = sha256("example")
var macIDs: [[String: Any]] = [["fingerprint": hex(example), "macID": MacID.make(fingerprint: example)],
                               ["fingerprint": hex(fp), "macID": MacID.make(fingerprint: fp)],
                               ["fingerprint": "00112233445566778899", "macID": MacID.make(fingerprint: unhex("00112233445566778899"))],
                               ["fingerprint": "001122334455667788", "macID": MacID.make(fingerprint: unhex("001122334455667788"))],
                               ["fingerprint": "ffffffffffffffffffff", "macID": MacID.make(fingerprint: unhex("ffffffffffffffffffff"))]]
macIDs = macIDs.map { var m = $0; m["wellFormed"] = MacID.isWellFormed(m["macID"] as! String); return m }
let wellFormed = ["0123456789ABCDEF", "0123456789abcdef", "0123456789ABCDEI", "0123456789ABCDE", "0123456789ABCDEFG", "ILOU0123456789AB"]
    .map { ["text": $0, "wellFormed": MacID.isWellFormed($0)] }

let p2 = try! P256.Signing.PrivateKey(rawRepresentation: unhex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721")).publicKey.x963Representation
let spki: [[String: Any]] = [pub, p2].map { ["point": hex($0), "der": hex(SPKI.der(point: $0)), "fingerprint": hex(SPKI.fingerprint(point: $0)),
                                             "macID": MacID.make(fingerprint: SPKI.fingerprint(point: $0)), "shortName": RemoteIdentity.shortName(SPKI.fingerprint(point: $0))] }

func addr(_ a: ParsedAddress) -> [String: Any] {
    ["host": a.host, "port": a.port.map { $0 as Any } ?? NSNull(), "kind": a.kind.rawValue, "text": a.text]
}
func failure(_ f: PairLink.Failure) -> String {
    switch f {
    case .notALink: return "notALink"
    case .version: return "version"
    case .field(let k): return "field:\(k)"
    case .macIDMismatch: return "macIDMismatch"
    case .address(let a): return "address:\(a)"
    }
}
let linkAddrs: [ParsedAddress] = ["100.101.102.103", "[fd7a:115c:a1e0::1234]:7456", "pc.tail1234.ts.net", "192.168.1.20", "10.0.0.2", "10.0.0.3"]
    .map { if case .success(let a) = AddressParser.parse($0) { return a }; fatalError($0) }
let links: [PairLink] = [
    PairLink(fingerprint: fp, secret: secret, name: "PC", port: 7455, addresses: Array(linkAddrs.prefix(3))),
    PairLink(fingerprint: fp, secret: secret, name: "Tom & Jerry = 100% ü/?#", port: 1, addresses: linkAddrs),
    PairLink(fingerprint: example, secret: Data(repeating: 0xff, count: 16), name: "  \u{202E}\n", port: 65535, addresses: []),
    PairLink(fingerprint: fp, secret: secret, name: String(repeating: "N", count: 70), port: 7455, addresses: []),
]
let linkURLs: [[String: Any]] = links.map { ["fingerprint": hex($0.fingerprint), "secret": hex($0.secret), "name": $0.name,
                                             "port": $0.port, "addresses": $0.addresses.map(addr), "url": $0.url, "macID": $0.macID] }
let good = links[0].url
let mid = MacID.make(fingerprint: fp), k = Base64URL.encode(fp), s = Base64URL.encode(secret)
var linkParses: [[String: Any]] = []
for text in [good, links[1].url, links[2].url, "  " + good + "\n", good.replacingOccurrences(of: "sill://pair", with: "SILL://PAIR"),
             good.replacingOccurrences(of: "sill://pair?", with: "sill://pair/?"),
             good.replacingOccurrences(of: "sill://pair?", with: "sill://pair/x?"),
             good.replacingOccurrences(of: "sill://pair?", with: "https://pair?"),
             good.replacingOccurrences(of: "sill://pair?", with: "sill://other?"),
             "sill://pair", "sill://pair?", "sill:pair?v=1", "not a link", "",
             "sill://pair?m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=2&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid.lowercased())&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)=&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(String(k.dropLast()))&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(Base64URL.encode(Data(repeating: 1, count: 15)))&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=0",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=65536",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=007455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=+7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=07455",
             "sill://pair?v=1&m=0123456789ABCDEF&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a=1.2.3",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a=",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a=%5Bfd7a::1%5D:80&a=host.example",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&n=Caf%C3%A9&n=Second&x=ignored",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&n=a+b",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&n=",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&n=bad%zz",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&n=sp ace",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455#frag",
             "sill://user@pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair:80?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a=1.1.1.1&a=1.1.1.2&a=1.1.1.3&a=1.1.1.4&a=1.1.1.5&a=1.1.1.6",
             "sill://pair?v=1&m=\(mid)&k=\(k)&s=\(s)&p=7455&a=1.1.1.1&a=1.1.1.2&a=1.1.1.3&a=1.1.1.4&a=1.1.1.5&a=bad..name",
             "sill://pair?V=1&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=1&&m=\(mid)&k=\(k)&s=\(s)&p=7455",
             "sill://pair?v=%31&m=\(mid)&k=\(k)&s=\(s)&p=7455"] {
        var e: [String: Any] = ["text": text]
        switch PairLink.parse(text) {
        case .success(let l):
            e["ok"] = ["macID": l.macID, "fingerprint": hex(l.fingerprint), "secret": hex(l.secret), "name": l.name, "port": l.port,
                       "addresses": l.addresses.map(addr)]
        case .failure(let f): e["failure"] = failure(f)
        }
        linkParses.append(e)
}

var doors: [[String: Any]] = []
for txt in [[:], ["p": "1"], ["p": "0"], ["P": "0"], ["p": ""], ["p": "yes"], ["p": "0", "P": "1"], ["P": "1", "p": "0"], ["r": "x"],
            ["p": " 0"], ["P": ""]] as [[String: String]] {
    let d: String
    switch HomeDoorTXT.door(txt) { case .plain: d = "plain"; case .pairingRequired: d = "pairingRequired"; case .open: d = "open" }
    doors.append(["txt": txt, "door": d])
}

write("pairing.json", [
    "damm": damm, "check": codeChecks, "grouped": grouped, "codeLength": PairingCode.length,
    "rounds": Int(PairingProof.rounds), "codeKeys": codeKeys,
    "proofs": [proofs("qr", qrK), proofs("code", codeK)],
    "recognitionTag": ["key": hex(rk), "nonce": "a1a2a3a4a5a6", "tag": tag, "key2": hex(Data(repeating: 0x33, count: 32)), "nonce2": "000000000000",
                       "tag2": tag2, "matches": tagMatches, "txtKey": RecognitionTag.txtKey],
    "base64url": ["encode": b64Encode, "decode": b64Decode],
    "macID": macIDs, "macIDWellFormed": wellFormed, "spki": spki,
    "pairLink": ["urls": linkURLs, "parse": linkParses, "maxAddresses": PairLink.maxAddresses, "nameLimit": PairLink.nameLimit],
    "homeDoorTXT": ["doors": doors, "valueOn": HomeDoorTXT.value(requirePairing: true), "valueOff": HomeDoorTXT.value(requirePairing: false),
                    "key": HomeDoorTXT.key],
    "alpn": ["session": RemoteTLS.sessionALPN, "pairing": RemoteTLS.pairingALPN, "server": RemoteTLS.serverALPNs, "serverName": RemoteTLS.serverName],
    "pairRequestVersion": PairRequest.version,
    "knownReasons": PairResult.knownReasons.sorted(),
])

// MARK: - text.json: addresses, versions, SafeText, viewport bounds

var addresses: [[String: Any]] = []
for input in ["100.101.102.103", "100.101.102.103:7455", " 100.64.0.1 ", "192.168.1.1:0", "192.168.1.1:65535", "192.168.1.1:65536",
              "192.168.1.1:00080", "192.168.1.1:000080", "192.168.1.1:", "192.168.1.1:abc", "192.168.1.1:+80", ":7455", "0.0.0.0",
              "255.255.255.255", "1.2.3", "1.2.3.4.5", "01.2.3.4", "00.1.2.3", "256.1.1.1", "1.2.3.4.", "123", "4294967295",
              "0x64.0x65.1.2", "0x1", "0X1", "0xg", "0x", "0x.1", "1.0x2", "017.1.1.1x", "0177.1", "08.1", "1e3", "100.1", "100.1. 1.1",
              "1.2.3.4x", "0xffffffff", "0x100000000", "0x1.0x1000000", "1.0x1000000", "1.2.0x10000", "1.2.0xffff",
              "[fd7a:115c:a1e0::1234]:7455", "[fd7a:115c:a1e0::1234]", "fd7a:115c:a1e0::1234", "FD7A:115C:A1E0:0:0:0:0:1234",
              "2001:DB8::0:1", "2001:db8:0:0:1:0:0:1", "2001:db8::1:0:0:1", "2001:0:0:1:0:0:0:1", "1:0:0:0:0:0:0:0", "0:0:0:0:0:0:0:1",
              "1:2:3:4:5:6:7:8", "1:2:3:4:5:6:0:8", "::", "[::]", "::1", "[::1]:80", "::ffff:192.168.1.1", "[::ffff:192.168.1.1]:80",
              "::ffff:c0a8:101", "::1.2.3.4", "::ffff:0:1.2.3.4", "64:ff9b::1.2.3.4", "fe80::1%en0", "[fe80::1%25en0]:80", "fd7a::1:7455",
              "1::2::3", "12345::1", "[fd7a::1", "[fd7a::1]x", "[fd7a::1]:", "[fd7a::1]:0", "[1.2.3.4]", "[]", "[pc.local]", "1:2",
              "mac-mini.tail1234.ts.net", "mac-mini.tail1234.ts.net:7455", "mac-mini", "localhost", "localhost.", "pc.local.", "-bad.com",
              "bad-.com", "a..b", ".a", "a_b.com", "ünïcode.com", "12.ts.net", "1.2.3.a", "a.1.2.3", String(repeating: "a", count: 63) + ".com",
              String(repeating: "a", count: 64) + ".com", Array(repeating: String(repeating: "a", count: 49), count: 5).joined(separator: "."),
              Array(repeating: String(repeating: "a", count: 50), count: 5).joined(separator: "."), "", "   ", "\n", "sill://pair?v=1", "SILL:x",
              "a\u{0}b", "a\u{7F}b", "a b", "a\u{00A0}b", "a\u{2028}b", "host:65536", "host:abc", "100.101.102.103%en0", "a%b",
              "xn--bcher-kva.example", "1.2.3.4:5:6", "host:port:x", "Mac-Mini.Tail1234.TS.NET", "\u{2003}100.1.1.1\u{2003}",
              "::2", "::0.1.0.0", "::5:0:0:0:0", "0:0:0:0:0:1:1.2.3.4", "::0.0.1.0", "::ffff:0:0", "1:2:3:4:5:6:7::", "::1:2:3:4:5:6:7",
              "1:2:3:4:5:6:7:8::", "1::2:3:4:5:6:7:8", "0001::", "00001::", ":1::2", "1::2:", "1:2:3:4:5:6:1.2.3.4", "::1.2.3", "::01.2.3.4",
              "1.2.3.4::", "::256.1.1.1", "g::1", "::ffff:1.2.3.4.5", "1:2:3:4:5:6:7:8:9", "::1.2.3.4:5", "[::1]:65535", "::fffe:1.2.3.4",
              "0x1.2.3.4", "0x100.1", "0xff.0xffffff", "0xff.0x1000000", "00x1", "0x0x1", "1.2.3.04x"] {
    var e: [String: Any] = ["input": input]
    switch AddressParser.parse(input) {
    case .success(let a): e["ok"] = addr(a)
    case .failure(let f): e["failure"] = String(describing: f)
    }
    addresses.append(e)
}

var versions: [[String: Any]] = []
for t in ["v0.4.0", "0.4", "0.4.0", "1.2.3-beta.1", "0.3.0 (85)", " 1.2 ", "V2", "vv1", "", ".1", "1..2", "1.", "1...", "1.2.3.4.5.6.7.8",
          "1.2.3.4.5.6.7.8.9", "1234567890", "123456789", "٣", "1.0", "0", "0.0.0", "1.2.3.4.5.6.7.8.", "01.02", "1.2a.3", "1.-2",
          " v1.2", "v 1.2", "\n0.9\n", "1.00000000001", "999999999.1", "x1"] {
    var e: [String: Any] = ["text": t]
    if let v = SillVersion(t) { e["components"] = v.components; e["description"] = v.description } else { e["components"] = NSNull() }
    versions.append(e)
}
var compares: [[String: Any]] = []
for (a, b) in [("0.10", "0.9"), ("1.0.1", "1.0"), ("v0.4.0", "0.4"), ("1", "1.0.0"), ("0", "0.0.1"), ("2.0", "10.0"), ("1.2", "1.10"),
               ("1.2.3", "1.2.3.0.1")] {
    let x = SillVersion(a)!, y = SillVersion(b)!
    compares.append(["a": a, "b": b, "order": x < y ? "less" : (x == y ? "equal" : "greater")])
}
let versionDescriptions = [[], [0, 4, 0], [1], [1, 0, 1], [2, 0, 0, 0]].map { ["components": $0, "description": SillVersion(components: $0).description] }

var labels: [[String: Any]] = []
func label(_ t: String, _ limit: Int? = nil) {
    labels.append(["text": t, "limit": limit.map { $0 as Any } ?? NSNull(), "label": limit.map { SafeText.label(t, limit: $0) } ?? SafeText.label(t)])
}
for t in ["iPad", "  Kamal's\tiPad\n", "a\u{202E}b", "\u{202A}x\u{202C}\u{2066}y\u{2069}", "\u{200B}zero\u{200D}width\u{FEFF}", "line\u{2028}sep\u{2029}para",
          String(repeating: "a", count: 100), String(repeating: "e\u{301}", count: 70), String(repeating: "👨‍👩‍👧", count: 65),
          "\u{0}\u{7}\u{1B}[31mred", "", "   ", "\u{E000}private", "\u{0378}unassigned", "a\u{00A0}\u{3000}b", "a \t \n b",
          String(repeating: "a", count: 63) + " b", "Noah’s MacBook Pro", "\r\nforged: line", "🇸🇩 flag", "a\u{85}b", "a\u{180E}b",
          "\u{1F468}\u{200D}\u{1F4BB}", "ㄱㅏ", "\u{AD}soft"] {
    label(t)
}
label("abcd efgh", 5)
label("abcdefgh", 0)
label("a b c", 3)
label(String(repeating: "x", count: 301), 300)

func vp(_ w: Double, _ h: Double, _ s: Double?, _ name: String) -> [String: Any] {
    let v = Viewport(width: w, height: h, scale: s)
    var e: [String: Any] = ["name": name, "widthBits": bits64(w), "heightBits": bits64(h), "scaleBits": s.map { bits64($0) as Any } ?? NSNull(),
                            "refusal": ViewportBounds.refusal(v).map { $0 as Any } ?? NSNull()]
    if let held = ViewportBounds.held(v) {
        e["held"] = ["widthBits": bits64(held.width), "heightBits": bits64(held.height), "scaleBits": held.scale.map { bits64($0) as Any } ?? NSNull()]
    } else { e["held"] = NSNull() }
    return e
}
let viewports = [vp(390, 844, nil, "phone"), vp(1, 16384, 0.25, "edges"), vp(0.5, 16385, 4.5, "outside"), vp(1e300, 1, 1e-300, "huge and tiny"),
                 vp(-1, 0, 0, "negative and zero"), vp(.nan, 10, .nan, "nan"), vp(.infinity, 10, -.infinity, "infinite"), vp(1512.7, 949.2, 1.5, "fractional"),
                 vp(0.1, 100, 0.1, "point one"), vp(100, 100, -2, "negative scale")]
let wholes: [[String: Any]] = [1512.7, -3.9, 0, 1e300, .nan, .infinity, -.infinity, 9.3e18, -0.0].map { ["xBits": bits64($0), "whole": ViewportBounds.whole($0)] }
let numbers: [[String: Any]] = [-1, 16385, 0.1, 1e-300, .nan, 1, 0.25, 1e300, 1e16, 123456.5, -0.0].map { ["xBits": bits64($0), "number": ViewportBounds.number($0)] }

write("text.json", [
    "addresses": addresses,
    "versions": ["parse": versions, "compare": compares, "describe": versionDescriptions],
    "safeText": ["labelLimit": SafeText.labelLimit, "labels": labels],
    "viewportBounds": ["viewports": viewports, "whole": wholes, "number": numbers],
])

print("make-vectors: wrote wire.json, json.json, pairing.json, text.json to \(outDir)")
