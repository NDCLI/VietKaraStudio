using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

internal static class UpdateHelper
{
    private sealed class CopiedFile
    {
        public string Target;
        public string Backup;
        public bool Existed;
    }

    [STAThread]
    private static int Main(string[] args)
    {
        string root = null;
        try
        {
            if (args.Length != 3) throw new InvalidOperationException("Tham số cập nhật không hợp lệ.");
            int processId;
            if (!Int32.TryParse(args[0], out processId)) throw new InvalidOperationException("Không xác định được ứng dụng đang chạy.");
            string payload = Path.GetFullPath(args[1]);
            root = Path.GetFullPath(args[2]);
            string appExe = Path.Combine(root, "VietKaraStudio.exe");
            if (!File.Exists(appExe) || !Directory.Exists(payload)) throw new InvalidOperationException("Không tìm thấy thư mục ứng dụng hoặc bản vá.");

            try { Process.GetProcessById(processId).WaitForExit(); }
            catch (ArgumentException) { }
            catch (InvalidOperationException) { }

            string updateFolder = Directory.GetParent(payload).FullName;
            string backup = Path.Combine(updateFolder, "rollback");
            if (Directory.Exists(backup)) Directory.Delete(backup, true);
            Directory.CreateDirectory(backup);
            List<CopiedFile> copied = new List<CopiedFile>();
            try
            {
                string prefix = payload.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
                foreach (string source in Directory.GetFiles(payload, "*", SearchOption.AllDirectories))
                {
                    string relative = source.Substring(prefix.Length).Replace(Path.AltDirectorySeparatorChar, Path.DirectorySeparatorChar);
                    if (!SafeRelative(relative)) throw new InvalidOperationException("Bản vá chứa đường dẫn không được phép.");
                    string target = Path.GetFullPath(Path.Combine(root, relative));
                    string rootPrefix = root.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
                    if (!target.StartsWith(rootPrefix, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Đường dẫn cập nhật vượt khỏi thư mục ứng dụng.");
                    string saved = Path.Combine(backup, relative);
                    bool existed = File.Exists(target);
                    if (existed)
                    {
                        Directory.CreateDirectory(Path.GetDirectoryName(saved));
                        File.Copy(target, saved, true);
                    }
                    copied.Add(new CopiedFile { Target = target, Backup = saved, Existed = existed });
                    Directory.CreateDirectory(Path.GetDirectoryName(target));
                    File.Copy(source, target, true);
                }
                if (!File.Exists(appExe)) throw new InvalidOperationException("Không tìm thấy chương trình sau khi áp dụng bản vá.");
            }
            catch
            {
                for (int i = copied.Count - 1; i >= 0; i--)
                {
                    CopiedFile item = copied[i];
                    try
                    {
                        if (item.Existed && File.Exists(item.Backup)) File.Copy(item.Backup, item.Target, true);
                        else if (File.Exists(item.Target)) File.Delete(item.Target);
                    }
                    catch { }
                }
                throw;
            }

            try { Directory.Delete(updateFolder, true); }
            catch { }
            Process.Start(new ProcessStartInfo { FileName = appExe, WorkingDirectory = root, UseShellExecute = true });
            return 0;
        }
        catch (Exception error)
        {
            try
            {
                MessageBox.Show("Không cài được bản vá VietKara Studio. Bản cũ đã được khôi phục nếu cần.\r\n\r\n" + error.Message,
                    "Cập nhật VietKara Studio", MessageBoxButtons.OK, MessageBoxIcon.Error);
                if (!String.IsNullOrEmpty(root))
                {
                    string appExe = Path.Combine(root, "VietKaraStudio.exe");
                    if (File.Exists(appExe)) Process.Start(new ProcessStartInfo { FileName = appExe, WorkingDirectory = root, UseShellExecute = true });
                }
            }
            catch { }
            return 1;
        }
    }

    private static bool SafeRelative(string path)
    {
        if (String.IsNullOrEmpty(path) || Path.IsPathRooted(path) || path.IndexOf(':') >= 0) return false;
        string normalized = path.Replace(Path.AltDirectorySeparatorChar, Path.DirectorySeparatorChar);
        foreach (string part in normalized.Split(Path.DirectorySeparatorChar))
            if (part == ".." || part == "." || part.Length == 0) return false;
        return normalized == "VietKaraStudio.exe" || normalized == "BUILD-INFO.json" ||
            normalized.StartsWith("static" + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) ||
            normalized.StartsWith("resources" + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
    }
}
