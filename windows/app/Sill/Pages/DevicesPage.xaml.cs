using System.Text.Json.Nodes;
using System;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Sill;

public sealed partial class DevicesPage : SillPage
{
    string key = "";

    public DevicesPage()
    {
        InitializeComponent();
    }

    protected override void Render(Status? s)
    {
        if (s is null) return;
        var k = string.Join("|", s.Devices.Select(d => d.Key + d.Connected + d.App + d.Name));
        if (k == key) return;
        key = k;
        List.Children.Clear();
        Empty.Visibility = s.Devices.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var d in s.Devices.OrderByDescending(d => d.Connected).ThenByDescending(d => d.PairedAt))
        {
            var name = Format.Device(d.Name, d.Model);
            var model = d.Model is null ? "" : d.Model.Replace(" simulator", " (simulator)");
            var detail = string.Join(" · ", new[] { Format.App(d.App), model, $"paired {Format.Ago(d.PairedAt, s.Now)}", $"key {d.Key[..6]}…" }.Where(x => x.Length > 0));
            var trailing = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            if (d.Connected)
            {
                trailing.Children.Add(Ui.Pill("Connected"));
                var k2 = d.Key;
                trailing.Children.Add(Ui.TextButton("Disconnect", () => Run(Req.Of("disconnect", "key", k2)), $"Disconnect {name}"));
            }
            var keyToRemove = d.Key;
            trailing.Children.Add(Ui.TextButton("Remove", () => Remove(keyToRemove, name), $"Remove {name}"));
            List.Children.Add(Ui.Row(Ui.DeviceGlyph(d.Model), name, detail, trailing));
        }
    }

    async void Remove(string deviceKey, string name)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = $"Remove {name}?",
            Content = "It will be disconnected, and it must pair again to connect to this PC.",
            PrimaryButtonText = "Remove",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary) Run(Req.Of("unpair", "key", deviceKey));
    }

    void OnPair(object sender, RoutedEventArgs e) => App.Current?.ShowMain("pair");
}
