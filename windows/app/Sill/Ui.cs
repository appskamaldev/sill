// Small builders for the Settings-style rows the pages show, and the base every page derives from.

using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;

namespace Sill;

/// A page that redraws from the status while it is shown.
public abstract class SillPage : Page
{
    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        StatusService.Shared.Updated += OnStatus;
        OnStatus();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        StatusService.Shared.Updated -= OnStatus;
        base.OnNavigatedFrom(e);
    }

    void OnStatus() => Render(StatusService.Shared.Current);

    protected abstract void Render(Status? s);

    /// Runs a command and shows its error, if any, in a dialog.
    protected async void Run(object request, string? failureTitle = null)
    {
        var e = await StatusService.Shared.RunAsync(request);
        if (e is not null && XamlRoot is not null)
        {
            await new ContentDialog
            {
                XamlRoot = XamlRoot,
                Title = failureTitle ?? "That didn't work",
                Content = e,
                CloseButtonText = "OK",
            }.ShowAsync();
        }
    }
}

public static class Ui
{
    public static Style S(string key) => (Style)Application.Current.Resources[key];
    public static Brush B(string key) => (Brush)Application.Current.Resources[key];

    public static FontIcon Glyph(string glyph, double size = 20) => new() { Glyph = glyph, FontSize = size };

    /// A card row: icon, title over a secondary line, then trailing content.
    public static Border Row(string glyph, string title, string? detail, UIElement? trailing = null, bool card = true)
    {
        var grid = new Grid { ColumnSpacing = 16, MinHeight = 44 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var icon = Glyph(glyph);
        icon.VerticalAlignment = VerticalAlignment.Center;
        grid.Children.Add(icon);
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Spacing = 1 };
        text.Children.Add(new TextBlock { Text = title, Style = S("BodyTextBlockStyle"), TextTrimming = TextTrimming.CharacterEllipsis });
        if (!string.IsNullOrEmpty(detail)) text.Children.Add(new TextBlock { Text = detail, Style = S("Secondary") });
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);
        if (trailing is FrameworkElement t)
        {
            t.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(t, 2);
            grid.Children.Add(t);
        }
        AutomationProperties.SetName(grid, detail is null ? title : $"{title}, {detail}");
        return new Border { Style = card ? S("Card") : null, Padding = card ? new Thickness(20, 12, 20, 12) : new Thickness(0, 8, 0, 8), Child = grid };
    }

    public static Button TextButton(string text, Action click, string? automation = null, bool accent = false)
    {
        var b = new Button { Content = text, MinWidth = 96 };
        if (accent) b.Style = S("AccentButtonStyle");
        if (automation is not null) AutomationProperties.SetName(b, automation);
        b.Click += (_, _) => click();
        return b;
    }

    public static Button IconButton(string glyph, string label, Action click)
    {
        var b = new Button { Content = Glyph(glyph, 14), Width = 36, Height = 32, Padding = new Thickness(0) };
        AutomationProperties.SetName(b, label);
        ToolTipService.SetToolTip(b, label);
        b.Click += (_, _) => click();
        return b;
    }

    /// The pill that says a device is connected.
    public static Border Pill(string text)
    {
        var tb = new TextBlock { Text = text, Style = S("CaptionTextBlockStyle"), Foreground = B("TextOnAccentFillColorPrimaryBrush") };
        return new Border { Background = B("SystemFillColorSuccessBrush"), CornerRadius = new CornerRadius(10), Padding = new Thickness(8, 2, 8, 3), Child = tb };
    }

    public static void Copy(string text)
    {
        var p = new Windows.ApplicationModel.DataTransfer.DataPackage();
        p.SetText(text);
        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(p);
    }

    /// The device's glyph: an iPad, or a phone.
    public static string DeviceGlyph(string? model) => model?.StartsWith("iPad", StringComparison.Ordinal) == true ? "" : "";
}
