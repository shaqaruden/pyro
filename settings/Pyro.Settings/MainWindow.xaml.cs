using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Pyro.Settings.Models;

namespace Pyro.Settings;

public sealed partial class MainWindow : Window
{
    private readonly string _configPath;
    private PyroConfig _config = new();

    public MainWindow()
    {
        InitializeComponent();
        _configPath = ResolveConfigPath();
        ConfigPathText.Text = $"Config file: {_configPath}";
        LoadConfig();
    }

    private static string ResolveConfigPath()
    {
        var args = Environment.GetCommandLineArgs();
        if (args.Length > 1 && !string.IsNullOrWhiteSpace(args[1]))
        {
            return Path.GetFullPath(args[1]);
        }

        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        return Path.Combine(appData, "pyro", "config.toml");
    }

    private void LoadConfig()
    {
        var (loaded, warning) = PyroConfigStore.Load(_configPath);
        _config = loaded;
        BindModelToControls();
        if (!string.IsNullOrWhiteSpace(warning))
        {
            ShowStatus(warning, Colors.OrangeRed);
        }
        else
        {
            ShowStatus("Loaded settings.", Colors.Gray);
        }
    }

    private void BindModelToControls()
    {
        CaptureHotkeyTextBox.Text = _config.CaptureHotkey;
        DefaultDelayNumberBox.Value = _config.DefaultDelayMs;
        SaveDirTextBox.Text = _config.SaveDir;
        CopyToClipboardCheckBox.IsChecked = _config.CopyToClipboard;
        OpenEditorCheckBox.IsChecked = _config.OpenEditor;
        TextCommitFeedbackColorTextBox.Text = _config.Editor.TextCommitFeedbackColor;

        SetTargetCombo(_config.DefaultTarget);

        var s = _config.Editor.Shortcuts;
        SelectShortcutTextBox.Text = s.Select;
        RectangleShortcutTextBox.Text = s.Rectangle;
        EllipseShortcutTextBox.Text = s.Ellipse;
        LineShortcutTextBox.Text = s.Line;
        ArrowShortcutTextBox.Text = s.Arrow;
        MarkerShortcutTextBox.Text = s.Marker;
        TextShortcutTextBox.Text = s.Text;
        PixelateShortcutTextBox.Text = s.Pixelate;
        BlurShortcutTextBox.Text = s.Blur;
        CopyShortcutTextBox.Text = s.Copy;
        SaveShortcutTextBox.Text = s.Save;
        CopyAndSaveShortcutTextBox.Text = s.CopyAndSave;
        UndoShortcutTextBox.Text = s.Undo;
        RedoShortcutTextBox.Text = s.Redo;
        DeleteSelectedShortcutTextBox.Text = s.DeleteSelected;
    }

    private void SetTargetCombo(string target)
    {
        for (var i = 0; i < DefaultTargetComboBox.Items.Count; i++)
        {
            if (DefaultTargetComboBox.Items[i] is ComboBoxItem item
                && item.Tag is string tag
                && string.Equals(tag, target, StringComparison.OrdinalIgnoreCase))
            {
                DefaultTargetComboBox.SelectedIndex = i;
                return;
            }
        }

        DefaultTargetComboBox.SelectedIndex = 0;
    }

    private bool TryApplyControlsToModel(out string? error)
    {
        error = null;

        var captureHotkey = CaptureHotkeyTextBox.Text.Trim();
        if (string.IsNullOrWhiteSpace(captureHotkey))
        {
            error = "Capture hotkey cannot be empty.";
            return false;
        }

        var selectedTarget = (DefaultTargetComboBox.SelectedItem as ComboBoxItem)?.Tag as string;
        if (string.IsNullOrWhiteSpace(selectedTarget))
        {
            error = "Default target selection is invalid.";
            return false;
        }

        var delayMs = (long)Math.Max(0, DefaultDelayNumberBox.Value);

        var saveDir = SaveDirTextBox.Text.Trim();
        if (string.IsNullOrWhiteSpace(saveDir))
        {
            error = "Save directory cannot be empty.";
            return false;
        }

        var color = TextCommitFeedbackColorTextBox.Text.Trim();
        if (!PyroConfigStore.IsValidHexColor(color))
        {
            error = "Text commit feedback color must be a hex value like #48B4FF.";
            return false;
        }

        var shortcuts = new Dictionary<string, string>
        {
            ["Select"] = SelectShortcutTextBox.Text.Trim(),
            ["Rectangle"] = RectangleShortcutTextBox.Text.Trim(),
            ["Ellipse"] = EllipseShortcutTextBox.Text.Trim(),
            ["Line"] = LineShortcutTextBox.Text.Trim(),
            ["Arrow"] = ArrowShortcutTextBox.Text.Trim(),
            ["Marker"] = MarkerShortcutTextBox.Text.Trim(),
            ["Text"] = TextShortcutTextBox.Text.Trim(),
            ["Pixelate"] = PixelateShortcutTextBox.Text.Trim(),
            ["Blur"] = BlurShortcutTextBox.Text.Trim(),
            ["Copy"] = CopyShortcutTextBox.Text.Trim(),
            ["Save"] = SaveShortcutTextBox.Text.Trim(),
            ["Copy+Save"] = CopyAndSaveShortcutTextBox.Text.Trim(),
            ["Undo"] = UndoShortcutTextBox.Text.Trim(),
            ["Redo"] = RedoShortcutTextBox.Text.Trim(),
            ["Delete Selected"] = DeleteSelectedShortcutTextBox.Text.Trim(),
        };

        foreach (var entry in shortcuts)
        {
            if (string.IsNullOrWhiteSpace(entry.Value))
            {
                error = $"Shortcut '{entry.Key}' cannot be empty.";
                return false;
            }
        }

        _config.CaptureHotkey = captureHotkey;
        _config.DefaultTarget = selectedTarget;
        _config.DefaultDelayMs = delayMs;
        _config.SaveDir = saveDir;
        _config.CopyToClipboard = CopyToClipboardCheckBox.IsChecked == true;
        _config.OpenEditor = OpenEditorCheckBox.IsChecked == true;
        _config.Editor.TextCommitFeedbackColor = PyroConfigStore.NormalizeHexColor(color);

        var s = _config.Editor.Shortcuts;
        s.Select = shortcuts["Select"];
        s.Rectangle = shortcuts["Rectangle"];
        s.Ellipse = shortcuts["Ellipse"];
        s.Line = shortcuts["Line"];
        s.Arrow = shortcuts["Arrow"];
        s.Marker = shortcuts["Marker"];
        s.Text = shortcuts["Text"];
        s.Pixelate = shortcuts["Pixelate"];
        s.Blur = shortcuts["Blur"];
        s.Copy = shortcuts["Copy"];
        s.Save = shortcuts["Save"];
        s.CopyAndSave = shortcuts["Copy+Save"];
        s.Undo = shortcuts["Undo"];
        s.Redo = shortcuts["Redo"];
        s.DeleteSelected = shortcuts["Delete Selected"];

        return true;
    }

    private void ShowStatus(string message, Windows.UI.Color color)
    {
        StatusText.Text = message;
        StatusText.Foreground = new SolidColorBrush(color);
    }

    private void SaveButton_Click(object sender, RoutedEventArgs e)
    {
        if (!TryApplyControlsToModel(out var error))
        {
            ShowStatus(error ?? "Invalid settings.", Colors.OrangeRed);
            return;
        }

        try
        {
            PyroConfigStore.Save(_configPath, _config);
            ShowStatus("Saved. Restart Pyro for defaults/hotkey changes to take effect.", Colors.LightGreen);
        }
        catch (Exception ex)
        {
            ShowStatus($"Save failed: {ex.Message}", Colors.OrangeRed);
        }
    }

    private void ReloadButton_Click(object sender, RoutedEventArgs e)
    {
        LoadConfig();
    }

    private void CloseButton_Click(object sender, RoutedEventArgs e)
    {
        Close();
    }
}
