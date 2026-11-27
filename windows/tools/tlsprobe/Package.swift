// swift-tools-version:5.9
// TLS probe for the Windows host: dials host:port with exactly the device's TLS options
// (RemoteTLS.options + RemoteTLS.parameters from upstream StreamProtocol, a fresh P-256 identity)
// and prints the NWConnection states and the OSStatus the device's dial policy would classify.
// Run: swift run -c release tlsprobe 127.0.0.1 17455   (see windows/TESTING.md, "TLS probe").
import PackageDescription
import Foundation

// The root package's identity is its directory's name (see tools/vectors/Package.swift).
let rootName = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .lastPathComponent.lowercased()

let package = Package(
    name: "SillTLSProbe",
    platforms: [.macOS(.v14)],
    dependencies: [.package(path: "../../..")],
    targets: [
        .executableTarget(name: "tlsprobe",
                          dependencies: [.product(name: "StreamProtocol", package: rootName)]),
    ]
)
