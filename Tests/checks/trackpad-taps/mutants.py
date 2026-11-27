"""Mutants of TrackpadTaps.swift (iOSClient): each changes it in one place and must make the check (main.swift)
fail. usage: mutants.py"""
import os, subprocess, sys
SP = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(SP, "..", "..", ".."))   # the repository
OUT = os.path.join(ROOT, ".build", "checks", "trackpad-taps")
os.makedirs(OUT, exist_ok=True)
SRC = os.path.join(ROOT, "iOSClient/TrackpadTaps.swift")
orig = open(SRC).read()
MUTANTS = [
    ("C1 a looser travel", "static let travel: CGFloat = 3", "static let travel: CGFloat = 10"),
    ("C2 a longer tap", "static let tapTime: TimeInterval = 0.35", "static let tapTime: TimeInterval = 0.5"),
    ("C3 a longer window", "static let dragWindow: TimeInterval = 0.3", "static let dragWindow: TimeInterval = 0.5"),
    ("E1 the travel's edge left out", "> Self.travel { moved = true }", ">= Self.travel { moved = true }"),
    ("E2 the tap time's edge left out", "time - start <= Self.tapTime", "time - start < Self.tapTime"),
    ("E3 a touch at the window's edge no drag", "strictly ? time > upAt : time >= upAt", "time >= upAt"),
    ("E4 the timer's own edge left out", "strictly ? time > upAt : time >= upAt", "time > upAt"),
    ("B1 the click sent at once (a double-click then a drag)", "            out.append(.leftDown)\n            buttonDown = true\n            upAt = time + Self.dragWindow\n",
     "            out.append(.leftDown)\n            out.append(.leftUp)\n            upAt = time + Self.dragWindow\n"),
    ("B2 no drag", "                upAt = nil\n                dragging = true\n", "                upAt = nil\n"),
    ("B3 the drag's up forgotten", "            buttonDown = false\n            out.append(.leftUp)\n", "            buttonDown = false\n"),
    ("B4 a moving stroke clicks", "let tap = !moved && !spoiled", "let tap = !spoiled"),
    ("B5 a cancel ignored", "let tap = !moved && !spoiled", "let tap = !moved"),
    ("B6 no right click", "            out.append(.rightClick)\n", "            break\n"),
    ("B7 three fingers right-click", "        case 2:\n", "        case 2, 3:\n"),
    ("B8 a cancelled drag held", "        dragging = false\n        buttonDown = false\n        return [.leftUp]\n", "        return []\n"),
    ("B9 reset forgets the button", "let out: [Send] = buttonDown ? [.leftUp] : []", "let out: [Send] = []"),
    ("B10 the first finger's lift decides", "guard landed.removeValue(forKey: id) != nil, landed.isEmpty, let start", "guard landed.removeValue(forKey: id) != nil, let start"),
    ("B11 the window from the touch, not the lift", "upAt = time + Self.dragWindow", "upAt = start + Self.dragWindow"),
]
caught = 0
for name, old, new in MUTANTS:
    if orig.count(old) != 1:
        print(f"{name}: NOT APPLIED (pattern found {orig.count(old)} times)"); continue
    src = os.path.join(OUT, "TrackpadTaps.swift")
    open(src, "w").write(orig.replace(old, new, 1))
    exe = os.path.join(OUT, "mutant")
    if os.path.exists(exe): os.remove(exe)
    c = subprocess.run(["swiftc", "-O", src, os.path.join(SP, "main.swift"), "-o", exe], capture_output=True, text=True)
    if c.returncode != 0:
        print(f"{name}: did not compile: {c.stderr.strip().splitlines()[0] if c.stderr.strip() else ''}"); continue
    r = subprocess.run([exe], capture_output=True, text=True, timeout=600)
    failed = [l for l in r.stdout.splitlines() if l.startswith("FAIL")]
    if r.returncode != 0:
        caught += 1
        print(f"{name}: caught ({len(failed)} FAIL lines; first: {failed[0][:150] if failed else 'exit ' + str(r.returncode)})")
    else:
        print(f"{name}: SURVIVED")
print(f"{caught} of {len(MUTANTS)} mutants caught")
sys.exit(0 if caught == len(MUTANTS) else 1)
