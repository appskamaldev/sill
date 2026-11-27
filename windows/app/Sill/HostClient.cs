// The host's control pipe (sill-net::control): one JSON line per request and reply, on
// \\.\pipe\sill-host-<this user's SID>. The host lets in only this user, and only a process whose
// exe lies in its folder (this app does: <host>\app\Sill.exe).

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using System.Threading;
using System.Threading.Tasks;

namespace Sill;

public sealed class Session
{
    public string Name { get; set; } = "";
    public string Key { get; set; } = "";
    public string Peer { get; set; } = "";
    public string Door { get; set; } = "";
    public string? App { get; set; }
    public double Since { get; set; }
}

public sealed class Device
{
    public string Key { get; set; } = "";
    public string Name { get; set; } = "";
    public string? Model { get; set; }
    public string? App { get; set; }
    public double PairedAt { get; set; }
    public string Method { get; set; } = "";
    public bool Connected { get; set; }
}

public sealed class Address
{
    public string Ip { get; set; } = "";
    public string Kind { get; set; } = "";
}

public sealed class Pairing
{
    public string Code { get; set; } = "";
    public string Link { get; set; } = "";
    public double EndsAt { get; set; }
    public List<string>? Qr { get; set; }
}

public sealed class HostSettings
{
    public bool RequirePairing { get; set; }
    public long Bitrate { get; set; }
    public long MaxFps { get; set; }
    public double CaptureScale { get; set; }
    public bool? StartAtLogin { get; set; }
}

public sealed class Stats
{
    public long Fps { get; set; }
    public long Kbps { get; set; }
    public double? LatencyMs { get; set; }
    public double? Cpu { get; set; }
    public double? MemoryMb { get; set; }
}

public sealed class Status
{
    public bool Ok { get; set; }
    public string Name { get; set; } = "";
    public string MacId { get; set; } = "";
    public string Version { get; set; } = "";
    public double Now { get; set; }
    public bool Paused { get; set; }
    public string Source { get; set; } = "";
    public string Picture { get; set; } = "";
    public List<Session> Sessions { get; set; } = new();
    public List<Device> Devices { get; set; } = new();
    public List<Address> Addresses { get; set; } = new();
    public int RemotePort { get; set; }
    public Pairing? Pairing { get; set; }
    public HostSettings Settings { get; set; } = new();
    public Stats Stats { get; set; } = new();
}

/// One connection to the host, reopened on demand.
public sealed class HostClient : IDisposable
{
    public static readonly HostClient Shared = new();

    static readonly JsonSerializerOptions Json = new()
    {
        PropertyNameCaseInsensitive = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    readonly SemaphoreSlim gate = new(1, 1);
    NamedPipeClientStream? pipe;
    StreamReader? reader;
    StreamWriter? writer;

    static string PipeName()
    {
        var sid = WindowsIdentity.GetCurrent().User?.Value ?? "";
        return "sill-host-" + sid;
    }

    /// The host exe beside this app's folder (<host>\app\Sill.exe → <host>\sill-host.exe).
    public static string HostExe =>
        Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "sill-host.exe"));

    public static bool HostInstalled => File.Exists(HostExe);

    /// Starts the host (hidden; it puts its own icon in the tray).
    public static bool StartHost()
    {
        try
        {
            if (!HostInstalled) return false;
            Process.Start(new ProcessStartInfo(HostExe) { UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Path.GetDirectoryName(HostExe)! });
            return true;
        }
        catch { return false; }
    }

    async Task<bool> EnsureAsync()
    {
        if (pipe is { IsConnected: true }) return true;
        Close();
        try
        {
            var p = new NamedPipeClientStream(".", PipeName(), PipeDirection.InOut, PipeOptions.Asynchronous);
            await p.ConnectAsync(400).ConfigureAwait(false);
            pipe = p;
            reader = new StreamReader(p, new UTF8Encoding(false));
            writer = new StreamWriter(p, new UTF8Encoding(false)) { AutoFlush = true, NewLine = "\n" };
            return true;
        }
        catch
        {
            Close();
            return false;
        }
    }

    void Close()
    {
        try { pipe?.Dispose(); } catch { }
        pipe = null; reader = null; writer = null;
    }

    /// Sends one request; null when the host cannot be reached.
    public async Task<JsonNode?> RequestAsync(object request)
    {
        await gate.WaitAsync().ConfigureAwait(false);
        try
        {
            for (var attempt = 0; attempt < 2; attempt++)
            {
                if (!await EnsureAsync().ConfigureAwait(false)) return null;
                try
                {
                    await writer!.WriteLineAsync(JsonSerializer.Serialize(request, Json)).ConfigureAwait(false);
                    var line = await reader!.ReadLineAsync().ConfigureAwait(false);
                    if (line is null) { Close(); continue; }
                    return JsonNode.Parse(line);
                }
                catch (IOException) { Close(); }
            }
            return null;
        }
        finally { gate.Release(); }
    }

    public async Task<Status?> StatusAsync()
    {
        var n = await RequestAsync(new { cmd = "status" }).ConfigureAwait(false);
        return n?.Deserialize<Status>(Json);
    }

    /// Sends a command; the error in words, or null when it went through.
    public async Task<string?> CommandAsync(object request)
    {
        var n = await RequestAsync(request).ConfigureAwait(false);
        if (n is null) return "The Sill host isn't running.";
        return n["ok"]?.GetValue<bool>() == true ? null : n["error"]?.GetValue<string>() ?? "Something went wrong.";
    }

    public async Task<List<string>> LogsAsync(int lines)
    {
        var n = await RequestAsync(new { cmd = "logs", lines }).ConfigureAwait(false);
        var list = new List<string>();
        if (n?["lines"] is JsonArray a)
            foreach (var l in a) if (l is not null) list.Add(l.GetValue<string>());
        return list;
    }

    public void Dispose() => Close();
}
