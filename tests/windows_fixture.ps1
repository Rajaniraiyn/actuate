param([ValidateSet('wpf','winforms')][string]$Toolkit = 'wpf', [switch]$Interactive)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class FixtureDesktop {
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    static extern bool GetUserObjectInformation(IntPtr handle, int index, StringBuilder value, int length, out int needed);
    public static void Verify() {
        var name = new StringBuilder(256);
        int needed;
        if (!GetUserObjectInformation(GetThreadDesktop(GetCurrentThreadId()), 2, name, 512, out needed)
            || !name.ToString().StartsWith("ActuateTest-"))
            throw new InvalidOperationException("Run fixtures through tests/windows_e2e.py on its isolated desktop");
    }
}
'@
if (-not $Interactive) { [FixtureDesktop]::Verify() }
# Run in Windows PowerShell with -STA. Windows show without activation.
Add-Type -AssemblyName PresentationFramework, WindowsBase, System.Windows.Forms, System.Drawing
if ($Toolkit -eq 'wpf') {
    $window = New-Object Windows.Window
    $window.Title = 'Actuate WPF fixture'
    $window.Width = 420; $window.Height = 320
    $window.Left = 60; $window.Top = 80
    $window.ShowActivated = $false
    $panel = New-Object Windows.Controls.StackPanel
    $window.Content = $panel
    $status = New-Object Windows.Controls.TextBlock
    $status.Text = 'count:0'
    $button = New-Object Windows.Controls.Button
    $button.Content = 'Increment'
    [Windows.Automation.AutomationProperties]::SetAutomationId($button, 'increment')
    $script:count = 0
    $button.Add_Click({ $script:count++; $status.Text = "count:$script:count" })
    $edit = New-Object Windows.Controls.TextBox
    [Windows.Automation.AutomationProperties]::SetAutomationId($edit, 'editor')
    $check = New-Object Windows.Controls.CheckBox
    $check.Content = 'Option'
    $panel.Children.Add($status) | Out-Null
    $panel.Children.Add($button) | Out-Null
    $panel.Children.Add($edit) | Out-Null
    $panel.Children.Add($check) | Out-Null
    if ($Interactive) {
        $window.Width = 560; $window.Height = 360
        $window.Left = 120; $window.Top = 160
        $window.Background = '#171B2E'; $window.Foreground = '#F3F4FF'
        $window.FontFamily = 'Segoe UI'; $window.FontSize = 18
        $panel.Margin = '28'
        $status.FontSize = 32; $status.Margin = '0,0,0,20'
        $button.Background = '#9B75F9'; $button.Foreground = '#171B2E'
        $button.Height = 45; $button.Margin = '0,0,0,14'
        $edit.Height = 40; $edit.Margin = '0,0,0,14'
        $edit.Text = 'Window-scoped cursor test'
        $tray = New-Object Windows.Forms.NotifyIcon
        $tray.Icon = [Drawing.SystemIcons]::Information
        $tray.Text = 'Actuate test tray'
        $trayMenu = New-Object Windows.Forms.ContextMenuStrip
        $trayItem = $trayMenu.Items.Add('Increment test counter')
        $trayItem.Add_Click({ $script:count++; $status.Text = "count:$script:count" })
        $trayMenu.Items.Add('Close menu') | Out-Null
        $tray.ContextMenuStrip = $trayMenu
        $tray.Visible = $true
        $window.Add_Closed({ $tray.Visible = $false; $tray.Dispose(); $trayMenu.Dispose() })
    }
    $second = New-Object Windows.Window
    $second.Title = 'Actuate WPF second window'
    $second.Width = 260; $second.Height = 120
    $second.Left = 500; $second.Top = 80
    $second.ShowActivated = $false
    if ($Interactive) {
        $second.Title = 'Actuate occlusion test'
        $second.Left = 740; $second.Top = 180
        $second.Width = 320; $second.Height = 200
        $second.Background = '#26334A'
        $label = New-Object Windows.Controls.TextBlock
        $label.Text = 'Cover window'; $label.FontSize = 26
        $label.Foreground = '#F3F4FF'; $label.Margin = '24'
        $second.Content = $label
    }
    $second.Show()
    $window.Add_Closed({ $second.Close(); [Windows.Threading.Dispatcher]::CurrentDispatcher.InvokeShutdown() })
    $window.Show()
    [Windows.Threading.Dispatcher]::Run()
} else {
    Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition @'
public class PassiveFixture : System.Windows.Forms.Form {
    protected override bool ShowWithoutActivation { get { return true; } }
}
'@
    $window = New-Object PassiveFixture
    $window.Text = 'Actuate WinForms fixture'
    $window.Width = 420; $window.Height = 320
    $window.StartPosition = 'Manual'; $window.Left = 60; $window.Top = 420
    $status = New-Object Windows.Forms.Label
    $status.Text = 'count:0'; $status.Top = 10; $status.Width = 300
    $button = New-Object Windows.Forms.Button
    $button.Text = 'Increment'; $button.Top = 40; $button.Name = 'increment'
    $script:count = 0
    $button.Add_Click({ $script:count++; $status.Text = "count:$script:count" })
    $edit = New-Object Windows.Forms.TextBox
    $edit.Top = 80; $edit.Width = 300; $edit.Name = 'editor'
    $check = New-Object Windows.Forms.CheckBox
    $check.Text = 'Option'; $check.Top = 120
    $window.Controls.AddRange(@($status,$button,$edit,$check))
    [Windows.Forms.Application]::Run($window)
}
