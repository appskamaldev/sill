// One instance: a second launch (the tray's click, Open Sill, Pair a Device…) is handed to the first,
// which shows the flyout or the window. The app ends when its last window closes; the host and its
// tray icon stay.

using System;
using System.Linq;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace Sill;

public partial class App : Application
{
    MainWindow? main;
    FlyoutWindow? flyout;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            Log("unhandled: " + e.Exception);
            e.Handled = true;
        };
    }

    /// The app's own small log, beside the host's.
    public static void Log(string text)
    {
        try
        {
            var dir = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Sill", "logs");
            System.IO.Directory.CreateDirectory(dir);
            System.IO.File.AppendAllText(System.IO.Path.Combine(dir, "app.log"), $"{DateTime.Now:yyyy-MM-dd HH:mm:ss} {text}\n");
        }
        catch { }
    }

    public static new App? Current => Application.Current as App;

    public DispatcherQueue? DispatcherQueue { get; private set; }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        DispatcherQueue = DispatcherQueue.GetForCurrentThread();
        StatusService.Shared.Demo = Environment.GetCommandLineArgs().Contains("--demo");
        StatusService.Shared.Start(DispatcherQueue);
        Route(Environment.GetCommandLineArgs().Skip(1).ToArray());
    }

    /// A forced theme ("--theme light|dark"); the system's otherwise.
    public ElementTheme Theme { get; private set; } = ElementTheme.Default;

    /// "--stay": the flyout stays open when it loses focus (for screenshots and tests).
    public bool Stay { get; private set; }

    static string? After(string[] args, string flag)
    {
        var i = Array.IndexOf(args, flag);
        return i >= 0 && i + 1 < args.Length ? args[i + 1] : null;
    }

    /// What a launch asks for: "--flyout", "--pair", "--page <tag>", or the window.
    public void Route(string[] args)
    {
        Theme = After(args, "--theme") switch { "light" => ElementTheme.Light, "dark" => ElementTheme.Dark, _ => Theme };
        Stay |= args.Contains("--stay");
        if (args.Contains("--flyout"))
        {
            ShowFlyout();
            return;
        }
        ShowMain(args.Contains("--pair") ? "pair" : After(args, "--page"));
    }

    public void ShowMain(string? page = null)
    {
        flyout?.Close();
        if (main is null)
        {
            main = new MainWindow();
            main.Closed += (_, _) => { main = null; ExitIfNone(); };
        }
        if (page is not null) main.Navigate(page);
        main.Show();
    }

    public void ShowFlyout()
    {
        if (flyout is not null)
        {
            // A second click on the tray icon closes it, as the system flyouts do.
            flyout.Close();
            return;
        }
        try
        {
            flyout = new FlyoutWindow();
            flyout.Closed += (_, _) => { flyout = null; ExitIfNone(); };
            flyout.Show();
        }
        catch (Exception e)
        {
            Log("flyout: " + e);
            flyout = null;
            ExitIfNone();
        }
    }

    void ExitIfNone()
    {
        if (main is null && flyout is null) Exit();
    }

    /// Activation handed over from a second launch.
    public void OnRedirected(AppActivationArguments a)
    {
        var line = a.Data is Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs l ? l.Arguments : "";
        Route(line.Split(' ', StringSplitOptions.RemoveEmptyEntries));
    }
}
