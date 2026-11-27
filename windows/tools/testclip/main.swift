// testclip: encodes the Windows host's synthetic test picture into windows/testdata/clip.bin.
//
// 1280×800, 30 frames at 30 fps: grey ramp, colour bars, a white bar moving left to right and the
// frame number. HEVC Main 8-bit through VideoToolbox, real time, no frame reordering, an IDR at
// frame 0 and frame 15 (a GOP of 15 that restarts with an IDR). Format: see
// windows/crates/sill-net/src/video.rs. Build and run (no Xcode project, no simulator):
//
//   swiftc -O windows/tools/testclip/main.swift -o /tmp/testclip && /tmp/testclip windows/testdata/clip.bin

import CoreMedia
import CoreText
import CoreGraphics
import Foundation
import VideoToolbox

let width = 1280, height = 800, fps = 30, frameCount = 30, gop = 15

guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write("usage: testclip OUTPUT\n".data(using: .utf8)!)
    exit(2)
}
let output = URL(fileURLWithPath: CommandLine.arguments[1])

func fail(_ s: String) -> Never {
    FileHandle.standardError.write("testclip: \(s)\n".data(using: .utf8)!)
    exit(1)
}

// MARK: Drawing

func draw(frame: Int, into buffer: CVPixelBuffer) {
    CVPixelBufferLockBaseAddress(buffer, [])
    defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
    guard let ctx = CGContext(data: CVPixelBufferGetBaseAddress(buffer), width: width, height: height, bitsPerComponent: 8,
                              bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpace(name: CGColorSpace.sRGB)!,
                              bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
    else { fail("no CGContext") }
    // Background and colour bars (top half).
    ctx.setFillColor(CGColor(red: 0.08, green: 0.09, blue: 0.11, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let bars: [(CGFloat, CGFloat, CGFloat)] = [(0.75, 0.75, 0.75), (0.75, 0.75, 0), (0, 0.75, 0.75), (0, 0.75, 0),
                                               (0.75, 0, 0.75), (0.75, 0, 0), (0, 0, 0.75)]
    let bw = CGFloat(width) / CGFloat(bars.count)
    for (i, c) in bars.enumerated() {
        ctx.setFillColor(CGColor(red: c.0, green: c.1, blue: c.2, alpha: 1))
        ctx.fill(CGRect(x: CGFloat(i) * bw, y: CGFloat(height) * 0.55, width: bw + 1, height: CGFloat(height) * 0.45))
    }
    // A grey ramp.
    for i in 0..<32 {
        let g = CGFloat(i) / 31
        ctx.setFillColor(CGColor(red: g, green: g, blue: g, alpha: 1))
        ctx.fill(CGRect(x: CGFloat(i) * CGFloat(width) / 32, y: CGFloat(height) * 0.42, width: CGFloat(width) / 32 + 1, height: 60))
    }
    // The moving bar: one full sweep per loop.
    let x = CGFloat(frame) / CGFloat(frameCount) * CGFloat(width - 80)
    ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    ctx.fill(CGRect(x: x, y: 40, width: 80, height: CGFloat(height) * 0.24))
    // The frame number and a label.
    let text = "Sill test picture  frame \(String(format: "%02d", frame))/\(frameCount)" as CFString
    let font = CTFontCreateWithName("Menlo-Bold" as CFString, 44, nil)
    let attrs = [kCTFontAttributeName: font, kCTForegroundColorAttributeName: CGColor(red: 1, green: 1, blue: 1, alpha: 1)] as CFDictionary
    let line = CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, text, attrs)!)
    ctx.textPosition = CGPoint(x: 60, y: CGFloat(height) * 0.32)
    CTLineDraw(line, ctx)
}

// MARK: Encoding

final class Sink {
    var parameterSets: [Data] = []
    var frames: [(key: Bool, au: Data)] = []
}
let sink = Sink()

var sessionOut: VTCompressionSession?
let status = VTCompressionSessionCreate(allocator: nil, width: Int32(width), height: Int32(height), codecType: kCMVideoCodecType_HEVC,
                                        encoderSpecification: [kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: true] as CFDictionary,
                                        imageBufferAttributes: nil, compressedDataAllocator: nil, outputCallback: nil,
                                        refcon: nil, compressionSessionOut: &sessionOut)
guard status == noErr, let session = sessionOut else { fail("VTCompressionSessionCreate: \(status)") }
let props: [CFString: Any] = [
    kVTCompressionPropertyKey_RealTime: true,
    kVTCompressionPropertyKey_ProfileLevel: kVTProfileLevel_HEVC_Main_AutoLevel,
    kVTCompressionPropertyKey_AllowFrameReordering: false,
    kVTCompressionPropertyKey_MaxKeyFrameInterval: gop,
    kVTCompressionPropertyKey_ExpectedFrameRate: fps,
    kVTCompressionPropertyKey_AverageBitRate: 6_000_000,
]
for (k, v) in props {
    let s = VTSessionSetProperty(session, key: k, value: v as CFTypeRef)
    if s != noErr { fail("VTSessionSetProperty \(k): \(s)") }
}
VTCompressionSessionPrepareToEncodeFrames(session)

func handle(_ sample: CMSampleBuffer) {
    let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[CFString: Any]]
    let notSync = attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool ?? false
    let key = !notSync
    if key, let fmt = CMSampleBufferGetFormatDescription(sample) {
        var count = 0
        CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(fmt, parameterSetIndex: 0, parameterSetPointerOut: nil,
                                                           parameterSetSizeOut: nil, parameterSetCountOut: &count, nalUnitHeaderLengthOut: nil)
        var sets: [Data] = []
        for i in 0..<count {
            var p: UnsafePointer<UInt8>?
            var n = 0
            CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(fmt, parameterSetIndex: i, parameterSetPointerOut: &p,
                                                               parameterSetSizeOut: &n, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil)
            if let p { sets.append(Data(bytes: p, count: n)) }
        }
        if sink.parameterSets.isEmpty { sink.parameterSets = sets } else if sink.parameterSets != sets { fail("parameter sets changed mid-clip") }
    }
    guard let block = CMSampleBufferGetDataBuffer(sample) else { fail("no data buffer") }
    var length = 0
    var ptr: UnsafeMutablePointer<CChar>?
    CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &ptr)
    var au = Data(bytes: ptr!, count: length)
    // Drop any in-band VPS/SPS/PPS/AUD (types 32-35): they travel as kind 0.
    var kept = Data()
    var i = 0
    while i + 4 <= au.count {
        let n = Int(au[i]) << 24 | Int(au[i + 1]) << 16 | Int(au[i + 2]) << 8 | Int(au[i + 3])
        let type = (au[i + 4] >> 1) & 0x3F
        if !(32...35).contains(type) { kept.append(au[i..<(i + 4 + n)]) }
        i += 4 + n
    }
    au = kept
    sink.frames.append((key, au))
}

for f in 0..<frameCount {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(nil, width, height, kCVPixelFormatType_32BGRA, [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary, &pb)
    guard let pb else { fail("CVPixelBufferCreate") }
    draw(frame: f, into: pb)
    let pts = CMTime(value: CMTimeValue(f), timescale: CMTimeScale(fps))
    let opts = f % gop == 0 ? [kVTEncodeFrameOptionKey_ForceKeyFrame: true] as CFDictionary : nil
    let s = VTCompressionSessionEncodeFrame(session, imageBuffer: pb, presentationTimeStamp: pts,
                                            duration: CMTime(value: 1, timescale: CMTimeScale(fps)),
                                            frameProperties: opts, infoFlagsOut: nil) { status, _, sample in
        guard status == noErr, let sample else { fail("encode: \(status)") }
        handle(sample)
    }
    if s != noErr { fail("VTCompressionSessionEncodeFrame: \(s)") }
}
VTCompressionSessionCompleteFrames(session, untilPresentationTimeStamp: .invalid)
VTCompressionSessionInvalidate(session)

guard sink.frames.count == frameCount, sink.frames[0].key, sink.parameterSets.count >= 3 else {
    fail("got \(sink.frames.count) frames, first key \(sink.frames.first?.key ?? false), \(sink.parameterSets.count) parameter sets")
}
for (i, f) in sink.frames.enumerated() where f.key != (i % gop == 0) {
    fail("frame \(i): keyframe \(f.key), expected \(i % gop == 0)")
}

// MARK: Writing

func be16(_ v: Int) -> [UInt8] { [UInt8(v >> 8 & 0xFF), UInt8(v & 0xFF)] }
func be32(_ v: Int) -> [UInt8] { [UInt8(v >> 24 & 0xFF), UInt8(v >> 16 & 0xFF), UInt8(v >> 8 & 0xFF), UInt8(v & 0xFF)] }

var ps = Data([4, UInt8(sink.parameterSets.count)])
for s in sink.parameterSets { ps.append(contentsOf: be16(s.count)); ps.append(s) }
var out = Data("SILLCLIP".utf8)
out.append(contentsOf: [1, UInt8(fps)] + be16(width) + be16(height) + be16(frameCount))
out.append(contentsOf: be32(ps.count)); out.append(ps)
for f in sink.frames {
    out.append(f.key ? 1 : 0)
    out.append(contentsOf: be32(f.au.count)); out.append(f.au)
}
try! out.write(to: output)
let sizes = sink.frames.map { $0.au.count }
print("wrote \(output.path): \(out.count) bytes, \(frameCount) frames (IDR every \(gop)), largest AU \(sizes.max()!) bytes")
