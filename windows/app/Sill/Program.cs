// The entry point: one Sill app per user. A second launch hands its arguments to the first and ends.

using System;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace Sill;

public static class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();
        if (Redirected().GetAwaiter().GetResult()) return 0;
        Application.Start(p =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return 0;
    }

    static async Task<bool> Redirected()
    {
        var current = AppInstance.GetCurrent().GetActivatedEventArgs();
        var key = AppInstance.FindOrRegisterForKey("Sill.App");
        if (key.IsCurrent)
        {
            key.Activated += (_, a) =>
            {
                var q = App.Current?.DispatcherQueue ?? DispatcherQueue.GetForCurrentThread();
                q?.TryEnqueue(() => App.Current?.OnRedirected(a));
            };
            return false;
        }
        await key.RedirectActivationToAsync(current);
        return true;
    }
}
