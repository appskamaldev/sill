// The few Win32 calls WinUI does not cover.

using System;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace Sill;

static class Native
{
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("dwmapi.dll")] static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);

    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);

    public static void Foreground(Window w) => SetForegroundWindow(Hwnd(w));

    public static IntPtr Hwnd(Window w) => WinRT.Interop.WindowNative.GetWindowHandle(w);

    public static AppWindow AppWindowOf(Window w) => AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(Hwnd(w)));

    public static double Scale(Window w)
    {
        var dpi = GetDpiForWindow(Hwnd(w));
        return dpi == 0 ? 1.0 : dpi / 96.0;
    }

    /// Sizes the window in DIPs.
    public static void Resize(Window w, double width, double height)
    {
        var s = Scale(w);
        AppWindowOf(w).Resize(new SizeInt32((int)Math.Round(width * s), (int)Math.Round(height * s)));
    }

    /// Applies the app's theme to a window's content and its caption buttons.
    public static void Theme(Window w, ElementTheme theme)
    {
        if (w.Content is FrameworkElement root) root.RequestedTheme = theme;
        var dark = theme == ElementTheme.Dark || (theme == ElementTheme.Default && Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var tb = AppWindowOf(w).TitleBar;
        tb.ButtonBackgroundColor = Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Colors.Transparent;
        tb.ButtonForegroundColor = dark ? Colors.White : Colors.Black;
        tb.ButtonHoverForegroundColor = dark ? Colors.White : Colors.Black;
        tb.ButtonHoverBackgroundColor = dark ? Windows.UI.Color.FromArgb(0x15, 0xFF, 0xFF, 0xFF) : Windows.UI.Color.FromArgb(0x0A, 0, 0, 0);
        tb.ButtonPressedBackgroundColor = dark ? Windows.UI.Color.FromArgb(0x0B, 0xFF, 0xFF, 0xFF) : Windows.UI.Color.FromArgb(0x06, 0, 0, 0);
        tb.ButtonInactiveForegroundColor = dark ? Windows.UI.Color.FromArgb(0x5D, 0xFF, 0xFF, 0xFF) : Windows.UI.Color.FromArgb(0x5C, 0, 0, 0);
    }

    /// Windows 11's rounded corners on a window without a frame.
    public static void Round(Window w)
    {
        int round = 2; // DWMWCP_ROUND
        DwmSetWindowAttribute(Hwnd(w), 33 /* DWMWA_WINDOW_CORNER_PREFERENCE */, ref round, sizeof(int));
    }
}
