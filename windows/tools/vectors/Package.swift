// swift-tools-version:5.9
// Golden-vector generator for the Windows host: compiles upstream's StreamProtocol (the repository
// root package, by path) and writes windows/vectors/*.json. Run through ../make-vectors.sh.
import PackageDescription
import Foundation

// A path dependency's identity is its directory's name, which differs between clones ("sill",
// "windows-app"), so it is read from where this manifest sits: windows/tools/vectors → the root.
let rootName = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .lastPathComponent.lowercased()

let package = Package(
    name: "SillVectors",
    platforms: [.macOS(.v14)],
    dependencies: [.package(path: "../../..")],
    targets: [
        .executableTarget(name: "make-vectors",
                          dependencies: [.product(name: "StreamProtocol", package: rootName)]),
    ]
)
