// Words for what the host reports.

using System;
using System.Globalization;
using System.Linq;

namespace Sill;

public static class Format
{
    public static readonly (long Bitrate, string Name)[] Presets =
    {
        (4_000_000, "Low"), (8_000_000, "Efficient"), (15_000_000, "Balanced"), (25_000_000, "High"),
        (40_000_000, "Pro"), (80_000_000, "Ultra"), (150_000_000, "Extreme"),
    };

    public static string Quality(long bitrate)
    {
        var p = Presets.FirstOrDefault(p => p.Bitrate == bitrate);
        var mbps = bitrate / 1_000_000.0;
        return p.Name is null ? $"Custom — {mbps:0.#} Mbps" : $"{p.Name} — {mbps:0.#} Mbps";
    }

    public static string Device(string name, string? model)
    {
        var kind = model is null ? null : model.StartsWith("iPad", StringComparison.Ordinal) ? "iPad" : model.StartsWith("iPhone", StringComparison.Ordinal) ? "iPhone" : null;
        if (model?.EndsWith("simulator", StringComparison.Ordinal) == true) kind = "Simulator";
        return string.IsNullOrWhiteSpace(name) ? kind ?? "Device" : name;
    }

    /// "Sill 0.6.2 · App Store" style: Dev builds have three digits of build, App Store four.
    public static string App(string? app) => app ?? "Sill";

    public static string Ago(double then, double now)
    {
        var s = Math.Max(0, now - then);
        if (s < 60) return "just now";
        if (s < 3600) return $"{(int)(s / 60)} min ago";
        if (s < 86400) return $"{(int)(s / 3600)} h ago";
        var d = DateTimeOffset.FromUnixTimeSeconds((long)then).LocalDateTime;
        return d.ToString("d MMM yyyy", CultureInfo.CurrentCulture);
    }

    public static string Duration(double seconds)
    {
        var t = TimeSpan.FromSeconds(Math.Max(0, seconds));
        return t.TotalHours >= 1 ? $"{(int)t.TotalHours} h {t.Minutes} min" : t.TotalMinutes >= 1 ? $"{t.Minutes} min" : "less than a minute";
    }

    public static string Countdown(double seconds)
    {
        var t = TimeSpan.FromSeconds(Math.Max(0, Math.Ceiling(seconds)));
        return $"{(int)t.TotalMinutes}:{t.Seconds:00}";
    }

    public static string Door(string door) => door == "home" ? "Home network" : "Remote (Tailscale or VPN)";
}
