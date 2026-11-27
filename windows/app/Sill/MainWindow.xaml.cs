using System;
using System.Linq;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Animation;

namespace Sill;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        var aw = Native.AppWindowOf(this);
        aw.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "sill.ico"));
        aw.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        // 1040 x 800, or less on a small screen (the pages scroll).
        var work = DisplayArea.GetFromWindowId(aw.Id, DisplayAreaFallback.Primary).WorkArea;
        var scale = Native.Scale(this);
        Native.Resize(this, System.Math.Min(1040, work.Width * 0.9 / scale), System.Math.Min(800, work.Height * 0.9 / scale));
        Center(aw);
        Navigate("status");
        // Start with focus on the selected page, without a focus rectangle: the rectangle shows
        // only once Tab or the arrow keys are used.
        Activated += FocusSelectedOnce;
        Nav.Loaded += (_, _) => DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () => (Nav.SelectedItem as Control)?.Focus(FocusState.Pointer));
        StatusService.Shared.Updated += OnStatus;
        Closed += (_, _) => StatusService.Shared.Updated -= OnStatus;
        OnStatus();
    }

    void FocusSelectedOnce(object sender, WindowActivatedEventArgs e)
    {
        if (e.WindowActivationState == WindowActivationState.Deactivated) return;
        Activated -= FocusSelectedOnce;
        DispatcherQueue.TryEnqueue(() => (Nav.SelectedItem as Control)?.Focus(FocusState.Pointer));
    }

    void OnStatus()
    {
        var s = StatusService.Shared;
        NoHost.Visibility = s.Reached && s.Current is null ? Visibility.Visible : Visibility.Collapsed;
        if (!HostClient.HostInstalled)
        {
            NoHostText.Text = "The Sill host (sill-host.exe) is not beside this app. Reinstall Sill.";
            StartButton.Visibility = Visibility.Collapsed;
        }
    }

    async void OnStartHost(object sender, RoutedEventArgs e)
    {
        StartButton.IsEnabled = false;
        HostClient.StartHost();
        for (var i = 0; i < 20 && StatusService.Shared.Current is null; i++)
        {
            await System.Threading.Tasks.Task.Delay(300);
            await StatusService.Shared.RefreshAsync();
        }
        StartButton.IsEnabled = true;
    }

    void Center(AppWindow aw)
    {
        var area = DisplayArea.GetFromWindowId(aw.Id, DisplayAreaFallback.Primary).WorkArea;
        aw.Move(new Windows.Graphics.PointInt32(area.X + (area.Width - aw.Size.Width) / 2, area.Y + (area.Height - aw.Size.Height) / 2));
    }

    public void Show()
    {
        Native.Theme(this, App.Current?.Theme ?? ElementTheme.Default);
        Activate();
        if (Native.AppWindowOf(this).Presenter is OverlappedPresenter p && p.State == OverlappedPresenterState.Minimized) p.Restore();
    }

    static Type PageFor(string tag) => tag switch
    {
        "pair" => typeof(PairPage),
        "devices" => typeof(DevicesPage),
        "streaming" => typeof(StreamingPage),
        "network" => typeof(NetworkPage),
        "logs" => typeof(LogsPage),
        "about" => typeof(AboutPage),
        _ => typeof(StatusPage),
    };

    public void Navigate(string tag)
    {
        var item = Nav.MenuItems.Concat(Nav.FooterMenuItems).OfType<NavigationViewItem>().FirstOrDefault(i => (string)i.Tag == tag);
        if (item is not null && !ReferenceEquals(Nav.SelectedItem, item)) Nav.SelectedItem = item;
        else Go(tag);
    }

    void Go(string tag)
    {
        var page = PageFor(tag);
        if (Pages.CurrentSourcePageType != page)
            Pages.Navigate(page, null, new EntranceNavigationTransitionInfo());
    }

    void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem i) Go((string)i.Tag);
    }
}
