Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$logPath = Join-Path $env:LOCALAPPDATA 'Carambola\diagnostics\taskbar-ui-automation.log'
$observerSource = @'
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Automation;

public static class CarambolaTaskbarUiaObserver
{
    private static readonly Stopwatch Clock = Stopwatch.StartNew();
    private static AutomationElement Root;
    private static string LogPath;
    private static StructureChangedEventHandler StructureHandler = OnStructureChanged;

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr FindWindow(string className, string windowName);

    [DllImport("kernel32.dll")]
    private static extern int GetCurrentProcessId();

    public static void Start(string logPath)
    {
        string directory = Path.GetDirectoryName(logPath);
        Directory.CreateDirectory(directory);
        LogPath = logPath;

        IntPtr taskbarHwnd = FindWindow("Shell_TrayWnd", null);
        if (taskbarHwnd == IntPtr.Zero)
        {
            throw new InvalidOperationException("FindWindow(Shell_TrayWnd) failed; Win32 error=" + Marshal.GetLastWin32Error());
        }

        Root = AutomationElement.FromHandle(taskbarHwnd);
        Write("UIA_START taskbar_hwnd=0x" + taskbarHwnd.ToInt64().ToString("X") + " pid=" + Root.Current.ProcessId);
        WriteSnapshot();
        Automation.AddStructureChangedEventHandler(Root, TreeScope.Subtree, StructureHandler);
        Write("UIA_HOOK_INSTALLED event=StructureChanged scope=Subtree");
    }

    public static void Stop()
    {
        if (Root != null)
        {
            try { Automation.RemoveStructureChangedEventHandler(Root, StructureHandler); }
            catch (Exception ex) { Write("UIA_HOOK_REMOVE_FAILED error=" + Clean(ex.Message)); }
        }
        Write("UIA_STOP");
        LogPath = null;
    }

    private static void WriteSnapshot()
    {
        try
        {
            AutomationElementCollection elements = Root.FindAll(TreeScope.Subtree, Condition.TrueCondition);
            int count = Math.Min(elements.Count, 512);
            Write("UIA_SNAPSHOT_BEGIN descendants=" + elements.Count + " logged=" + count);
            for (int i = 0; i < count; i++)
            {
                WriteElement("UIA_ELEMENT", elements[i]);
            }
            Write("UIA_SNAPSHOT_END");
        }
        catch (Exception ex)
        {
            Write("UIA_SNAPSHOT_FAILED error=" + Clean(ex.Message));
        }
    }

    private static void OnStructureChanged(object sender, StructureChangedEventArgs args)
    {
        try
        {
            AutomationElement element = sender as AutomationElement;
            Write("UIA_STRUCTURE_CHANGED change=" + args.StructureChangeType + " runtime_id=" + RuntimeId(args.GetRuntimeId()));
            if (element != null)
            {
                WriteElement("UIA_CHANGED_ELEMENT", element);
            }
        }
        catch (Exception ex)
        {
            Write("UIA_EVENT_READ_FAILED error=" + Clean(ex.Message));
        }
    }

    private static void WriteElement(string eventName, AutomationElement element)
    {
        try
        {
            AutomationElement.AutomationElementInformation current = element.Current;
            string controlType = current.ControlType == null ? "unknown" : current.ControlType.ProgrammaticName;
            Write(eventName + " name=" + Quote(current.Name) + " class=" + Quote(current.ClassName) +
                " automation_id=" + Quote(current.AutomationId) + " control_type=" + Quote(controlType) +
                " native_hwnd=0x" + current.NativeWindowHandle.ToString("X") + " pid=" + current.ProcessId +
                " offscreen=" + current.IsOffscreen);
        }
        catch (Exception ex)
        {
            Write(eventName + " details_unavailable=" + Quote(Clean(ex.Message)));
        }
    }

    private static string RuntimeId(int[] ids)
    {
        if (ids == null) return "unavailable";
        return String.Join(".", ids);
    }

    private static string Quote(string value)
    {
        return "\"" + Clean(value ?? "") + "\"";
    }

    private static string Clean(string value)
    {
        return (value ?? "").Replace("\r", " ").Replace("\n", " ").Replace("\"", "'");
    }

    private static void Write(string message)
    {
        string path = LogPath;
        if (path == null) return;
        string line = "utc=" + DateTimeOffset.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ") +
            " elapsed_ms=" + Clock.ElapsedMilliseconds + " observer_pid=" + GetCurrentProcessId() + " " + message;
        try
        {
            File.AppendAllText(path, line + Environment.NewLine);
        }
        catch (IOException) { }
    }
}
'@

$references = [AppDomain]::CurrentDomain.GetAssemblies() |
    ForEach-Object { try { $_.Location } catch { $null } } |
    Where-Object { $_ -and (Test-Path -LiteralPath $_) } |
    Select-Object -Unique
Add-Type -TypeDefinition $observerSource -ReferencedAssemblies $references

try {
    [CarambolaTaskbarUiaObserver]::Start($logPath)
    Write-Host "Watching Explorer taskbar UI Automation events. Log: $logPath"
    Read-Host 'Reproduce the flash, then press Enter to stop the observer' | Out-Null
}
finally {
    [CarambolaTaskbarUiaObserver]::Stop()
}
