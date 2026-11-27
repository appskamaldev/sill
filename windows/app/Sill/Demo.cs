// "--demo": made-up status for screenshots and design review, so no real PC name, address, key or
// pairing secret ends up in a picture. The QR is a pattern with finder squares, not a real code.

using System;
using System.Collections.Generic;
using System.Text;

namespace Sill;

static class Demo
{
    public static Status Status()
    {
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        return new Status
        {
            Ok = true,
            Name = "STUDIO-PC",
            MacId = "DEMO0000DEMO0000",
            Version = "0.1.0",
            Now = now,
            Source = "desktop",
            Sessions = new() { new Session { Name = "iPad Pro", Key = "demo-ipad-key-0001", Peer = "192.168.1.40", Door = "home", App = "Sill 0.6.3", Since = now - 1260 } },
            Devices = new()
            {
                new Device { Key = "demo-ipad-key-0001", Name = "iPad Pro", Model = "iPad16,5", App = "Sill 0.6.3", PairedAt = now - 86400 * 3, Method = "code", Connected = true },
                new Device { Key = "demo-iphone-key-02", Name = "iPhone", Model = "iPhone17,1", App = "Sill 0.6.3", PairedAt = now - 86400 * 9, Method = "qr", Connected = false },
            },
            Addresses = new() { new Address { Ip = "100.100.10.20", Kind = "tailscale" }, new Address { Ip = "192.168.1.20", Kind = "lan" } },
            RemotePort = 7455,
            Pairing = new Pairing { Code = "4821 0937 6154", Link = "demo", EndsAt = now + 272, Qr = Qr(29) },
            Settings = new HostSettings { RequirePairing = true, Bitrate = 25_000_000, MaxFps = 60, CaptureScale = 2, StartAtLogin = true },
            Stats = new Stats { Fps = 60, Kbps = 18400, LatencyMs = 21, Cpu = 3.4, MemoryMb = 41 },
        };
    }

    static List<string> Qr(int n)
    {
        var rng = new Random(7);
        var g = new bool[n, n];
        for (var y = 0; y < n; y++) for (var x = 0; x < n; x++) g[y, x] = rng.Next(2) == 1;
        void Finder(int ox, int oy)
        {
            for (var y = -1; y <= 7; y++) for (var x = -1; x <= 7; x++)
            {
                int px = ox + x, py = oy + y;
                if (px < 0 || py < 0 || px >= n || py >= n) continue;
                var ring = Math.Max(Math.Abs(x - 3), Math.Abs(y - 3));
                g[py, px] = ring != 2 && ring != 4;
            }
        }
        Finder(0, 0); Finder(n - 7, 0); Finder(0, n - 7);
        var rows = new List<string>();
        for (var y = 0; y < n; y++)
        {
            var sb = new StringBuilder();
            for (var x = 0; x < n; x++) sb.Append(g[y, x] ? '1' : '0');
            rows.Add(sb.ToString());
        }
        return rows;
    }
}
