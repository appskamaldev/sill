using System.Linq;
using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;

namespace Sill;

public sealed partial class PairPage : SillPage
{
    string link = "";
    string addressesKey = "";

    public PairPage()
    {
        InitializeComponent();
    }

    protected override void Render(Status? s)
    {
        if (s is null) return;
        var p = s.Pairing;
        Closed.Visibility = p is null ? Visibility.Visible : Visibility.Collapsed;
        Open.Visibility = p is null ? Visibility.Collapsed : Visibility.Visible;
        if (p is not null)
        {
            CodeText.Text = p.Code;
            var left = p.EndsAt - s.Now;
            ExpiresText.Text = $"Expires in {Format.Countdown(left)}";
            ExpiresBar.Value = System.Math.Clamp(left, 0, 300);
            if (p.Link != link)
            {
                link = p.Link;
                DrawQr(p);
            }
            AutomationProperties.SetName(QrBox, $"Pairing QR code, code {p.Code}");
        }
        else link = "";

        var key = string.Join("|", s.Addresses.Select(a => a.Ip)) + s.RemotePort;
        if (key == addressesKey) return;
        addressesKey = key;
        AddressList.Children.Clear();
        foreach (var a in s.Addresses.Where(a => !a.Ip.Contains(':')))
        {
            var (title, detail, glyph) = a.Kind == "tailscale"
                ? ("Tailscale", "Works from anywhere on your tailnet", "")
                : ("Home network", "Found on this Wi‑Fi without typing; or type this address", "");
            var row = new Microsoft.UI.Xaml.Controls.StackPanel { Orientation = Microsoft.UI.Xaml.Controls.Orientation.Horizontal, Spacing = 12 };
            row.Children.Add(new Microsoft.UI.Xaml.Controls.TextBlock
            {
                Text = a.Ip, FontFamily = (FontFamily)Application.Current.Resources["MonoFont"],
                VerticalAlignment = VerticalAlignment.Center, IsTextSelectionEnabled = true,
            });
            var ip = a.Ip;
            row.Children.Add(Ui.IconButton("", $"Copy {ip}", () => Ui.Copy(ip)));
            AddressList.Children.Add(Ui.Row(glyph, title, detail, row));
        }
    }

    /// The host's QR modules as one path, a unit square per dark module, runs merged.
    void DrawQr(Pairing p)
    {
        var rows = p.Qr;
        if (rows is null || rows.Count == 0) { QrPath.Data = null; return; }
        var n = rows.Count;
        var sb = new StringBuilder();
        for (var y = 0; y < n; y++)
        {
            var r = rows[y];
            var x = 0;
            while (x < r.Length)
            {
                if (r[x] != '1') { x++; continue; }
                var start = x;
                while (x < r.Length && r[x] == '1') x++;
                sb.Append($"M{start},{y}h{x - start}v1h{-(x - start)}z");
            }
        }
        QrCanvas.Width = n;
        QrCanvas.Height = n;
        QrPath.Data = (Geometry)XamlBindingHelper.ConvertValue(typeof(Geometry), sb.ToString());
    }

    void OnStart(object sender, RoutedEventArgs e) => Run(new { cmd = "pair" }, "Pairing could not start");

    void OnStop(object sender, RoutedEventArgs e) => Run(new { cmd = "cancelPairing" });

    void OnCopyCode(object sender, RoutedEventArgs e)
    {
        var code = StatusService.Shared.Current?.Pairing?.Code;
        if (code is not null) Ui.Copy(code.Replace(" ", ""));
    }
}
