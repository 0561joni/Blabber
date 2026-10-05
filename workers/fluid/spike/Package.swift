// swift-tools-version: 6.2
// Throwaway phase 0 measurement CLI for the live pair. Not bundled.
import PackageDescription

let package = Package(
    name: "FluidSpike",
    platforms: [.macOS(.v14)],
    dependencies: [
        // v0.17.5. The NemoTextProcessing trait (a prebuilt binary) is disabled.
        .package(
            url: "https://github.com/FluidInference/FluidAudio.git",
            revision: "0b1f46289fe27d95b5e66ad8be46e64f5ee02ae7",
            traits: []
        )
    ],
    targets: [
        .executableTarget(
            name: "FluidSpike",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")]
        )
    ]
)
