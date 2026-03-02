using System.Text.RegularExpressions;
using Tomlyn;
using Tomlyn.Model;

namespace Pyro.Settings.Models;

public sealed class PyroConfig
{
    public string CaptureHotkey { get; set; } = "PrintScreen";
    public string DefaultTarget { get; set; } = "region";
    public long DefaultDelayMs { get; set; } = 0;
    public bool CopyToClipboard { get; set; } = true;
    public bool OpenEditor { get; set; } = true;
    public string SaveDir { get; set; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
        "Pyro"
    );
    public EditorConfig Editor { get; set; } = new();
}

public sealed class EditorConfig
{
    public string TextCommitFeedbackColor { get; set; } = "#48B4FF";
    public EditorShortcutConfig Shortcuts { get; set; } = new();
}

public sealed class EditorShortcutConfig
{
    public string Select { get; set; } = "S";
    public string Rectangle { get; set; } = "R";
    public string Ellipse { get; set; } = "E";
    public string Line { get; set; } = "L";
    public string Arrow { get; set; } = "A";
    public string Marker { get; set; } = "M";
    public string Text { get; set; } = "T";
    public string Pixelate { get; set; } = "P";
    public string Blur { get; set; } = "B";
    public string Copy { get; set; } = "Ctrl+C";
    public string Save { get; set; } = "Ctrl+S";
    public string CopyAndSave { get; set; } = "Ctrl+Shift+S";
    public string Undo { get; set; } = "Ctrl+Z";
    public string Redo { get; set; } = "Ctrl+Y";
    public string DeleteSelected { get; set; } = "Delete";
}

public static class PyroConfigStore
{
    private static readonly Regex HexColorRegex = new("^#?[0-9A-Fa-f]{6}$", RegexOptions.Compiled);

    public static (PyroConfig Config, string? Warning) Load(string path)
    {
        var config = new PyroConfig();
        if (!File.Exists(path))
        {
            return (config, null);
        }

        try
        {
            var source = File.ReadAllText(path);
            if (Toml.ToModel(source) is not TomlTable root)
            {
                return (config, "Config parse returned empty model; defaults loaded.");
            }

            config.CaptureHotkey = ReadString(root, "capture_hotkey", config.CaptureHotkey);
            config.DefaultTarget = ReadString(root, "default_target", config.DefaultTarget);
            config.DefaultDelayMs = ReadInt64(root, "default_delay_ms", config.DefaultDelayMs);
            config.CopyToClipboard = ReadBool(root, "copy_to_clipboard", config.CopyToClipboard);
            config.OpenEditor = ReadBool(root, "open_editor", config.OpenEditor);
            config.SaveDir = ReadString(root, "save_dir", config.SaveDir);

            var editor = ReadTable(root, "editor");
            config.Editor.TextCommitFeedbackColor = ReadString(
                editor,
                "text_commit_feedback_color",
                config.Editor.TextCommitFeedbackColor
            );

            var shortcuts = ReadTable(editor, "shortcuts");
            config.Editor.Shortcuts.Select = ReadString(shortcuts, "select", config.Editor.Shortcuts.Select);
            config.Editor.Shortcuts.Rectangle = ReadString(shortcuts, "rectangle", config.Editor.Shortcuts.Rectangle);
            config.Editor.Shortcuts.Ellipse = ReadString(shortcuts, "ellipse", config.Editor.Shortcuts.Ellipse);
            config.Editor.Shortcuts.Line = ReadString(shortcuts, "line", config.Editor.Shortcuts.Line);
            config.Editor.Shortcuts.Arrow = ReadString(shortcuts, "arrow", config.Editor.Shortcuts.Arrow);
            config.Editor.Shortcuts.Marker = ReadString(shortcuts, "marker", config.Editor.Shortcuts.Marker);
            config.Editor.Shortcuts.Text = ReadString(shortcuts, "text", config.Editor.Shortcuts.Text);
            config.Editor.Shortcuts.Pixelate = ReadString(shortcuts, "pixelate", config.Editor.Shortcuts.Pixelate);
            config.Editor.Shortcuts.Blur = ReadString(shortcuts, "blur", config.Editor.Shortcuts.Blur);
            config.Editor.Shortcuts.Copy = ReadString(shortcuts, "copy", config.Editor.Shortcuts.Copy);
            config.Editor.Shortcuts.Save = ReadString(shortcuts, "save", config.Editor.Shortcuts.Save);
            config.Editor.Shortcuts.CopyAndSave = ReadString(
                shortcuts,
                "copy_and_save",
                config.Editor.Shortcuts.CopyAndSave
            );
            config.Editor.Shortcuts.Undo = ReadString(shortcuts, "undo", config.Editor.Shortcuts.Undo);
            config.Editor.Shortcuts.Redo = ReadString(shortcuts, "redo", config.Editor.Shortcuts.Redo);
            config.Editor.Shortcuts.DeleteSelected = ReadString(
                shortcuts,
                "delete_selected",
                config.Editor.Shortcuts.DeleteSelected
            );

            if (!HexColorRegex.IsMatch(config.Editor.TextCommitFeedbackColor))
            {
                config.Editor.TextCommitFeedbackColor = "#48B4FF";
            }

            return (config, null);
        }
        catch (Exception ex)
        {
            return (config, $"Failed to parse config: {ex.Message}. Defaults loaded.");
        }
    }

    public static void Save(string path, PyroConfig config)
    {
        TomlTable root;
        if (File.Exists(path))
        {
            try
            {
                root = Toml.ToModel(File.ReadAllText(path)) as TomlTable ?? new TomlTable();
            }
            catch
            {
                root = new TomlTable();
            }
        }
        else
        {
            root = new TomlTable();
        }

        root["capture_hotkey"] = config.CaptureHotkey;
        root["default_target"] = config.DefaultTarget;
        root["default_delay_ms"] = config.DefaultDelayMs;
        root["copy_to_clipboard"] = config.CopyToClipboard;
        root["open_editor"] = config.OpenEditor;
        root["save_dir"] = config.SaveDir;

        var editor = ReadOrCreateTable(root, "editor");
        editor["text_commit_feedback_color"] = NormalizeHexColor(config.Editor.TextCommitFeedbackColor);

        var shortcuts = ReadOrCreateTable(editor, "shortcuts");
        shortcuts["select"] = config.Editor.Shortcuts.Select;
        shortcuts["rectangle"] = config.Editor.Shortcuts.Rectangle;
        shortcuts["ellipse"] = config.Editor.Shortcuts.Ellipse;
        shortcuts["line"] = config.Editor.Shortcuts.Line;
        shortcuts["arrow"] = config.Editor.Shortcuts.Arrow;
        shortcuts["marker"] = config.Editor.Shortcuts.Marker;
        shortcuts["text"] = config.Editor.Shortcuts.Text;
        shortcuts["pixelate"] = config.Editor.Shortcuts.Pixelate;
        shortcuts["blur"] = config.Editor.Shortcuts.Blur;
        shortcuts["copy"] = config.Editor.Shortcuts.Copy;
        shortcuts["save"] = config.Editor.Shortcuts.Save;
        shortcuts["copy_and_save"] = config.Editor.Shortcuts.CopyAndSave;
        shortcuts["undo"] = config.Editor.Shortcuts.Undo;
        shortcuts["redo"] = config.Editor.Shortcuts.Redo;
        shortcuts["delete_selected"] = config.Editor.Shortcuts.DeleteSelected;

        var serialized = Toml.FromModel(root);
        var parent = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(parent))
        {
            Directory.CreateDirectory(parent);
        }
        File.WriteAllText(path, serialized);
    }

    public static bool IsValidHexColor(string value)
    {
        return HexColorRegex.IsMatch(value.Trim());
    }

    public static string NormalizeHexColor(string value)
    {
        var trimmed = value.Trim();
        if (!trimmed.StartsWith('#'))
        {
            trimmed = "#" + trimmed;
        }
        return trimmed.ToUpperInvariant();
    }

    private static TomlTable ReadOrCreateTable(TomlTable root, string key)
    {
        if (root.TryGetValue(key, out var value) && value is TomlTable table)
        {
            return table;
        }

        var created = new TomlTable();
        root[key] = created;
        return created;
    }

    private static TomlTable ReadTable(TomlTable root, string key)
    {
        if (root.TryGetValue(key, out var value) && value is TomlTable table)
        {
            return table;
        }

        return new TomlTable();
    }

    private static string ReadString(TomlTable table, string key, string fallback)
    {
        if (!table.TryGetValue(key, out var value) || value is null)
        {
            return fallback;
        }

        return value switch
        {
            string s => s,
            _ => fallback,
        };
    }

    private static bool ReadBool(TomlTable table, string key, bool fallback)
    {
        if (!table.TryGetValue(key, out var value) || value is null)
        {
            return fallback;
        }

        return value switch
        {
            bool b => b,
            _ => fallback,
        };
    }

    private static long ReadInt64(TomlTable table, string key, long fallback)
    {
        if (!table.TryGetValue(key, out var value) || value is null)
        {
            return fallback;
        }

        return value switch
        {
            long i => i,
            int i => i,
            double d => (long)d,
            _ => fallback,
        };
    }
}
