using System.Text.Json.Nodes;
// Polls the host once a second while any window is open, and tells the pages.

using System;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;

namespace Sill;

public sealed class StatusService
{
    public static readonly StatusService Shared = new();

    DispatcherQueueTimer? timer;
    bool busy;
    DateTime? startedHostAt;

    /// The last status; null when the host is not running.
    public Status? Current { get; private set; }
    public bool Reached { get; private set; }

    /// "--demo": made-up status, no host (screenshots).
    public bool Demo { get; set; }

    /// Raised on the UI thread after every poll.
    public event Action? Updated;

    public void Start(DispatcherQueue queue)
    {
        if (timer is not null) return;
        timer = queue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(1);
        timer.Tick += async (_, _) => await RefreshAsync();
        timer.Start();
        _ = RefreshAsync();
    }

    public async Task RefreshAsync()
    {
        if (Demo)
        {
            Current = Sill.Demo.Status();
            Reached = true;
            Updated?.Invoke();
            return;
        }
        if (busy) return;
        busy = true;
        try
        {
            var s = await HostClient.Shared.StatusAsync();
            // Not running: start it once, and give it a few seconds before saying so.
            if (s is null && startedHostAt is null && HostClient.HostInstalled && HostClient.StartHost())
                startedHostAt = DateTime.UtcNow;
            Current = s;
            Reached = s is not null || startedHostAt is null || DateTime.UtcNow - startedHostAt > TimeSpan.FromSeconds(8);
            Updated?.Invoke();
        }
        finally { busy = false; }
    }

    /// Sends a command, then polls at once so the window shows its effect.
    public async Task<string?> RunAsync(JsonObject request)
    {
        if (Demo) return null;
        var e = await HostClient.Shared.CommandAsync(request);
        await RefreshAsync();
        return e;
    }
}
