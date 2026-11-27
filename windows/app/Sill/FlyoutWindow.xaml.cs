// The tray flyout: what Sill is doing, the devices, and the few things to do from the tray. It sits
// above the notification area and closes when it loses focus, like the system's own flyouts.

using System.Linq;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Graphics;

namespace Sill;

public sealed partial class FlyoutWindow : Window
{
    const double Width_ = 360;
    string key = "";
    bool closing;
    bool wasActive;

    public FlyoutWindow()
    {
        InitializeComponent();
        var aw = Native.AppWindowOf(this);
        var p = OverlappedPresenter.Create();
        p.SetBorderAndTitleBar(true, false);
        p.IsResizable = false;
        p.IsMaximizable = false;
        p.IsMinimizable = false;
        p.IsAlwaysOnTop = true;
        aw.SetPresenter(p);
        aw.IsShownInSwitchers = false;
        Native.Round(this);
        Activated += (_, e) =>
        {
            if (e.WindowActivationState != WindowActivationState.Deactivated) wasActive = true;
            else if (wasActive && App.Current?.Stay != true) CloseOnce();
        };
        StatusService.Shared.Updated += Render;
        Closed += (_, _) => StatusService.Shared.Updated -= Render;
        Render();
    }

    void CloseOnce()
    {
        if (closing) return;
        closing = true;
        Close();
    }

    public void Show()
    {
        Place(460);
        if (Content is FrameworkElement root) root.RequestedTheme = App.Current?.Theme ?? ElementTheme.Default;
        Activate();
        Native.Foreground(this);
        PairButton.Focus(FocusState.Programmatic);
    }

    /// Bottom right of the work area of the display under the pointer, 12 px in from each edge.
    void Place(double height)
    {
        var aw = Native.AppWindowOf(this);
        var s = Native.Scale(this);
        var w = (int)(Width_ * s);
        var h = (int)(height * s);
        var area = DisplayArea.GetFromPoint(new PointInt32(0, 0), DisplayAreaFallback.Primary).WorkArea;
        var margin = (int)(12 * s);
        aw.MoveAndResize(new RectInt32(area.X + area.Width - w - margin, area.Y + area.Height - h - margin, w, h));
    }

    void Render()
    {
        var s = StatusService.Shared.Current;
        if (s is null)
        {
            StateText.Text = "Sill isn't running";
            Dot.Fill = (Brush)Application.Current.Resources["SystemFillColorCriticalBrush"];
            PauseButton.Content = "Start Sill";
            PcName.Text = "";
            return;
        }
        PcName.Text = s.Name;
        if (s.Paused)
        {
            StateText.Text = "Hosting is paused";
            Dot.Fill = (Brush)Application.Current.Resources["SystemFillColorCautionBrush"];
        }
        else if (s.Sessions.Count > 0)
        {
            StateText.Text = "Streaming to " + string.Join(", ", s.Sessions.Select(x => x.Name).Distinct());
            Dot.Fill = (Brush)Application.Current.Resources["SystemFillColorSuccessBrush"];
        }
        else
        {
            StateText.Text = s.Pairing is null ? "Ready for your devices" : $"Pairing open · code {s.Pairing.Code}";
            Dot.Fill = (Brush)Application.Current.Resources["AccentFillColorDefaultBrush"];
        }
        PauseButton.Content = s.Paused ? "Resume hosting" : "Pause hosting";

        var k = string.Join("|", s.Devices.Select(d => d.Key + d.Connected + d.App));
        if (k == key) return;
        key = k;
        DeviceList.Children.Clear();
        NoDevices.Visibility = s.Devices.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var d in s.Devices.OrderByDescending(d => d.Connected).ThenByDescending(d => d.PairedAt).Take(4))
        {
            var name = Format.Device(d.Name, d.Model);
            var detail = d.Connected ? $"{Format.App(d.App)} · Connected now" : $"{Format.App(d.App)} · paired {Format.Ago(d.PairedAt, s.Now)}";
            UIElement? trailing = null;
            if (d.Connected)
            {
                var k2 = d.Key;
                var b = Ui.IconButton("", $"Disconnect {name}", async () => await StatusService.Shared.RunAsync(new { cmd = "disconnect", key = k2 }));
                trailing = b;
            }
            var row = Ui.Row(Ui.DeviceGlyph(d.Model), name, detail, trailing, card: false);
            row.Padding = new Thickness(12, 6, 8, 6);
            row.CornerRadius = new CornerRadius(6);
            if (d.Connected) row.Background = (Brush)Application.Current.Resources["SubtleFillColorSecondaryBrush"];
            DeviceList.Children.Add(row);
        }
    }

    void OnPair(object sender, RoutedEventArgs e)
    {
        App.Current?.ShowMain("pair");
    }

    void OnOpen(object sender, RoutedEventArgs e) => App.Current?.ShowMain();

    async void OnPause(object sender, RoutedEventArgs e)
    {
        var s = StatusService.Shared.Current;
        if (s is null)
        {
            HostClient.StartHost();
            return;
        }
        await StatusService.Shared.RunAsync(new { cmd = s.Paused ? "resume" : "pause" });
    }

    async void OnQuit(object sender, RoutedEventArgs e)
    {
        await HostClient.Shared.CommandAsync(new { cmd = "quit" });
        CloseOnce();
    }

    void OnKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Escape) CloseOnce();
    }
}
