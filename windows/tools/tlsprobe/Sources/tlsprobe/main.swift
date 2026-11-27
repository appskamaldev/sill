// tlsprobe HOST PORT [--alpn=sill/1] [--expect=revoked|ready] [--wait=SECONDS]
//
// Dials like the device's RemoteConnector: DeviceTLS's pin rule with no pin (any P-256 key, as
// on the typed pairing path; the server's verdict on our key is the point), a fresh identity,
// RemoteTLS.parameters(dialing: true). Prints each state with the time since start, and after
// .ready one receive and its error. The last line is the device's verdict:
//   RESULT revoked  tls(-9825|-9829) or tlsAfterReady(-9825|-9829): "Not paired"
//   RESULT notSill  any other TLS status
//   RESULT ready    the session opened and the host sent data
//   RESULT other    anything else (posix, timeout)
// --expect makes the exit status 0 only for that verdict.
import Foundation
import Network
import StreamProtocol

let args = CommandLine.arguments.dropFirst()
let positional = args.filter { !$0.hasPrefix("--") }
guard positional.count == 2, let port = NWEndpoint.Port(positional[positional.startIndex + 1]) else {
    FileHandle.standardError.write("usage: tlsprobe HOST PORT [--alpn=sill/1] [--expect=revoked|notSill|ready] [--wait=S]\n".data(using: .utf8)!)
    exit(2)
}
func option(_ name: String) -> String? {
    args.first { $0.hasPrefix("--\(name)=") }.map { String($0.dropFirst(name.count + 3)) }
}
let host = positional[positional.startIndex]
let alpn = option("alpn") ?? RemoteTLS.sessionALPN
let expect = option("expect")
let wait = Double(option("wait") ?? "10") ?? 10

guard let key = RemoteKey.generate(), let identity = RemoteIdentity(privateKey: key) else {
    print("no identity"); exit(2)
}
print("probe key \(RemoteIdentity.shortName(identity.fingerprint))… (base64url \(Base64URL.encode(identity.fingerprint).prefix(8))…)")

let queue = DispatchQueue(label: "tlsprobe")
let start = Date()
func t() -> String { String(format: "%7.1f ms", Date().timeIntervalSince(start) * 1000) }

let tls = RemoteTLS.options(identity: identity.tls, role: .client(alpn: alpn), verify: { fp in fp != nil }, queue: queue)
let connection = NWConnection(host: NWEndpoint.Host(host), port: port, using: RemoteTLS.parameters(tls: tls, dialing: true))

func describe(_ e: NWError) -> String {
    switch e {
    case .tls(let s): return "tls(\(s))"
    case .posix(let p): return "posix(\(p.rawValue) \(p))"
    case .dns(let d): return "dns(\(d))"
    default: return "\(e)"
    }
}
func finish(_ verdict: String) -> Never {
    print("RESULT \(verdict)")
    connection.cancel()
    exit(expect == nil || expect == verdict ? 0 : 1)
}
func verdict(_ e: NWError, afterReady: Bool) -> String {
    if let s = RemoteTLS.status(of: e) {
        return (s == -9825 || s == -9829) ? "revoked" : "notSill"
    }
    return "other"
}

var wasReady = false
func receive() {
    connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, done, error in
        if let error {
            print("\(t())  receive error \(describe(error)) status \(RemoteTLS.status(of: error).map(String.init) ?? "nil")")
            finish(verdict(error, afterReady: true))
        }
        if let data, !data.isEmpty {
            print("\(t())  received \(data.count) bytes")
            finish("ready")
        }
        if done { print("\(t())  EOF"); finish("other") }
        receive()
    }
}

connection.stateUpdateHandler = { state in
    switch state {
    case .setup: print("\(t())  setup")
    case .preparing: print("\(t())  preparing")
    case .ready:
        wasReady = true
        print("\(t())  ready (alpn \(RemoteTLS.negotiatedALPN(connection) ?? "none"))")
        receive()
    case .waiting(let e):
        print("\(t())  waiting \(describe(e)) status \(RemoteTLS.status(of: e).map(String.init) ?? "nil")")
        // The device's connector ends a TLS wait at once (RemoteConnector.end).
        if RemoteTLS.status(of: e) != nil { finish(verdict(e, afterReady: false)) }
    case .failed(let e):
        print("\(t())  failed \(describe(e)) status \(RemoteTLS.status(of: e).map(String.init) ?? "nil")")
        finish(verdict(e, afterReady: wasReady))
    case .cancelled: print("\(t())  cancelled")
    @unknown default: print("\(t())  \(state)")
    }
}
connection.start(queue: queue)
queue.asyncAfter(deadline: .now() + wait) { print("\(t())  timeout"); finish("other") }
dispatchMain()
