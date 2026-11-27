using System.Text.Json.Nodes;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Sill;

public sealed partial class NetworkPage : SillPage
{
    bool updating;
    string key = "";

    public NetworkPage()
    {
        InitializeComponent();
    }

    protected override void Render(Status? s)
    {
        if (s is null) return;
        updating = true;
        RequirePairing.IsOn = s.Settings.RequirePairing;
        StartAtLogin.IsOn = s.Settings.StartAtLogin == true;
        StartAtLogin.IsEnabled = s.Settings.StartAtLogin is not null;
        updating = false;

        var k = string.Join("|", s.Addresses.Select(a => a.Ip)) + s.RemotePort;
        if (k == key) return;
        key = k;
        AddressList.Children.Clear();
        foreach (var a in s.Addresses)
        {
            var v6 = a.Ip.Contains(':');
            var title = a.Kind == "tailscale" ? (v6 ? "Tailscale (IPv6)" : "Tailscale") : (v6 ? "Home network (IPv6)" : "Home network");
            var detail = a.Kind == "tailscale" ? "From anywhere on your tailnet" : "On this Wi‑Fi or Ethernet";
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
            row.Children.Add(new TextBlock { Text = a.Ip, FontFamily = (Microsoft.UI.Xaml.Media.FontFamily)Application.Current.Resources["MonoFont"], VerticalAlignment = VerticalAlignment.Center, IsTextSelectionEnabled = true });
            var ip = a.Ip;
            row.Children.Add(Ui.IconButton("", $"Copy {ip}", () => Ui.Copy(ip)));
            AddressList.Children.Add(Ui.Row(a.Kind == "tailscale" ? "" : "", title, detail, row));
        }
        AddressList.Children.Add(Ui.Row("", "Port", "Devices use it with any address above; type it only if you changed it.",
            new TextBlock { Text = s.RemotePort.ToString(), FontFamily = (Microsoft.UI.Xaml.Media.FontFamily)Application.Current.Resources["MonoFont"] }));
    }

    void OnRequirePairing(object sender, RoutedEventArgs e)
    {
        if (!updating) Run(Req.Of("set", "requirePairing", RequirePairing.IsOn));
    }

    void OnStartAtLogin(object sender, RoutedEventArgs e)
    {
        if (!updating) Run(Req.Of("set", "startAtLogin", StartAtLogin.IsOn));
    }
}
