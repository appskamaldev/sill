using System.Reflection;

namespace Sill;

public sealed partial class AboutPage : SillPage
{
    public AboutPage()
    {
        InitializeComponent();
    }

    protected override void Render(Status? s)
    {
        var app = Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "?";
        VersionText.Text = s is null ? $"App {app}" : $"App {app} · Host {s.Version} · This PC's ID {s.MacId}";
    }
}
