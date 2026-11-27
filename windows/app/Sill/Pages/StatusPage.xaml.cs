using System.Text.Json.Nodes;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Sill;

public sealed partial class StatusPage : SillPage
{
    string sessionsKey = "";

    public StatusPage()
    {
        InitializeComponent();
    }

    protected override void Render(Status? s)
    {
        if (s is null) return;
        var streaming = s.Sessions.Count > 0;
        if (s.Paused)
        {
            StateTitle.Text = "Hosting is paused";
            StateDetail.Text = "Devices can't connect until you resume.";
            StateIcon.Glyph = "";
            StateDisc.Fill = Ui.B("SystemFillColorCautionBrush");
        }
        else if (streaming)
        {
            var names = string.Join(", ", s.Sessions.Select(x => Format.Device(x.Name, null)).Distinct());
            StateTitle.Text = $"Streaming to {names}";
            StateDetail.Text = s.Source.StartsWith("window:") ? "A window of this PC is on the device." : "This PC's screen is on the device.";
            StateIcon.Glyph = "";
            StateDisc.Fill = Ui.B("SystemFillColorSuccessBrush");
        }
        else
        {
            StateTitle.Text = "Ready for your devices";
            StateDetail.Text = s.Devices.Count == 0
                ? "Pair your iPhone or iPad to see and control this PC from it."
                : "Open Sill on your iPhone or iPad to see and control this PC.";
            StateIcon.Glyph = "";
            StateDisc.Fill = Ui.B("AccentFillColorDefaultBrush");
        }
        PauseText.Text = s.Paused ? "Resume" : "Pause";
        PauseGlyph.Glyph = s.Paused ? "" : "";

        StatsGrid.Visibility = streaming ? Visibility.Visible : Visibility.Collapsed;
        FpsText.Text = $"{s.Stats.Fps} fps";
        LatencyText.Text = s.Stats.LatencyMs is double l ? $"{l:0} ms" : "–";
        RateText.Text = s.Stats.Kbps >= 1000 ? $"{s.Stats.Kbps / 1000.0:0.0} Mbps" : $"{s.Stats.Kbps} kbps";
        CostText.Text = $"{s.Stats.Cpu ?? 0:0}% · {s.Stats.MemoryMb ?? 0:0} MB";

        var key = string.Join("|", s.Sessions.Select(x => x.Key + x.App + x.Door));
        if (key != sessionsKey)
        {
            sessionsKey = key;
            SessionList.Children.Clear();
            foreach (var x in s.Sessions)
            {
                var device = s.Devices.FirstOrDefault(d => d.Key == x.Key);
                var detail = $"{Format.App(x.App)} · {Format.Door(x.Door)} · connected {Format.Ago(x.Since, s.Now)}";
                var key2 = x.Key;
                var name = Format.Device(x.Name, device?.Model);
                SessionList.Children.Add(Ui.Row(Ui.DeviceGlyph(device?.Model), name, detail,
                    Ui.TextButton("Disconnect", () => Run(Req.Of("disconnect", "key", key2)), $"Disconnect {name}")));
            }
        }
        NoSessions.Visibility = streaming ? Visibility.Collapsed : Visibility.Visible;

        NameText.Text = s.Name;
        SourceText.Text = s.Source switch
        {
            "desktop" => "The whole screen",
            "none" => "Nothing",
            var w when w.StartsWith("window:") => "One window",
            _ => s.Source,
        };
        AddressText.Text = string.Join("   ", s.Addresses.Where(a => !a.Ip.Contains(':')).Select(a => a.Ip));
    }

    void OnPause(object sender, RoutedEventArgs e)
    {
        var paused = StatusService.Shared.Current?.Paused == true;
        Run(Req.Of(paused ? "resume" : "pause"));
    }

    void OnPair(object sender, RoutedEventArgs e) => (App.Current)?.ShowMain("pair");
}
