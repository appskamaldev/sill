using System.Text.Json.Nodes;
using System.Linq;
using Microsoft.UI.Xaml.Controls;

namespace Sill;

public sealed partial class StreamingPage : SillPage
{
    bool updating;

    public StreamingPage()
    {
        InitializeComponent();
        foreach (var (bitrate, _) in Format.Presets)
            QualityBox.Items.Add(new ComboBoxItem { Content = Format.Quality(bitrate), Tag = bitrate });
    }

    protected override void Render(Status? s)
    {
        if (s is null) return;
        updating = true;
        Select(QualityBox, s.Settings.Bitrate.ToString());
        Select(ScaleBox, s.Settings.CaptureScale >= 1.5 ? "2" : "1");
        Select(FpsBox, s.Settings.MaxFps.ToString());
        updating = false;
    }

    static void Select(ComboBox box, string tag)
    {
        var item = box.Items.OfType<ComboBoxItem>().FirstOrDefault(i => i.Tag?.ToString() == tag);
        if (item is not null && !ReferenceEquals(box.SelectedItem, item)) box.SelectedItem = item;
    }

    static string? TagOf(ComboBox box) => (box.SelectedItem as ComboBoxItem)?.Tag?.ToString();

    void OnQuality(object sender, SelectionChangedEventArgs e)
    {
        if (!updating && long.TryParse(TagOf(QualityBox), out var b)) Run(Req.Of("set", "bitrate", b));
    }

    void OnScale(object sender, SelectionChangedEventArgs e)
    {
        if (!updating && double.TryParse(TagOf(ScaleBox), out var s)) Run(Req.Of("set", "captureScale", s));
    }

    void OnFps(object sender, SelectionChangedEventArgs e)
    {
        if (!updating && long.TryParse(TagOf(FpsBox), out var f)) Run(Req.Of("set", "maxFps", f));
    }
}
