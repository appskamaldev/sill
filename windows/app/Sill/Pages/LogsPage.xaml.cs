using System;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;
using Microsoft.UI.Xaml;

namespace Sill;

public sealed partial class LogsPage : SillPage
{
    bool loading;
    string last = "";

    public LogsPage()
    {
        InitializeComponent();
    }

    /// "2026-11-27T20:54:04.669836Z  INFO text" → "20:54:04  text", local time; warnings marked.
    static string Tidy(string line)
    {
        var m = Regex.Match(line, @"^(\S+)\s+(INFO|WARN|ERROR|DEBUG)\s+(.*)$");
        if (!m.Success) return line;
        var when = DateTimeOffset.TryParse(m.Groups[1].Value, out var t) ? t.LocalDateTime.ToString("HH:mm:ss") : m.Groups[1].Value;
        var level = m.Groups[2].Value switch { "WARN" => "  ⚠ ", "ERROR" => "  ✕ ", _ => "    " };
        return when + level + m.Groups[3].Value;
    }

    protected override async void Render(Status? s)
    {
        if (s is null || loading || !Follow.IsOn) return;
        loading = true;
        try
        {
            var lines = await HostClient.Shared.LogsAsync(400);
            var text = string.Join("\n", lines.ConvertAll(Tidy));
            if (text != last)
            {
                last = text;
                LogText.Text = text;
                Scroller.UpdateLayout();
                Scroller.ChangeView(0, Scroller.ScrollableHeight, null, true);
            }
        }
        finally { loading = false; }
    }

    void OnCopy(object sender, RoutedEventArgs e) => Ui.Copy(LogText.Text);

    void OnOpenFolder(object sender, RoutedEventArgs e)
    {
        var dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Sill", "logs");
        if (Directory.Exists(dir)) Process.Start(new ProcessStartInfo("explorer.exe", $"\"{dir}\"") { UseShellExecute = true });
    }
}
