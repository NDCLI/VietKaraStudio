using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using System.Windows.Forms;
[assembly: AssemblyTitle("VietKara Studio")]
[assembly: AssemblyProduct("VietKara Studio")]
[assembly: AssemblyDescription("Karaoke studio with original beat and synchronized lyrics")]
[assembly: AssemblyVersion("1.0.0.0")]
[assembly: AssemblyFileVersion("1.0.0.0")]
internal static class Launcher {
    private static string Quote(string s) {
        return "\"" + s.Replace("\"", "\\\"") + "\"";
    }
    [STAThread]
    private static int Main(string[] args) {
        bool created;
        using (Mutex mutex = new Mutex(true, "Local\\VietKaraStudio_1_0", out created)) {
            if (!created) { MessageBox.Show("VietKara Studio đang chạy. Hãy mở lại cửa sổ đang có.", "VietKara Studio"); return 0; }
            try {
                string root = AppDomain.CurrentDomain.BaseDirectory;
                string python = Path.Combine(root, "runtime", "pythonw.exe");
                if (!File.Exists(python)) throw new FileNotFoundException("Thiếu runtime. Hãy giải nén toàn bộ ZIP hoặc cài lại Setup.");
                StringBuilder arguments = new StringBuilder(Quote(Path.Combine(root, "bootstrap.py")));
                foreach (string arg in args) arguments.Append(" ").Append(Quote(arg));
                ProcessStartInfo start = new ProcessStartInfo(python, arguments.ToString());
                start.WorkingDirectory = root;
                start.UseShellExecute = false;
                start.CreateNoWindow = true;
                start.EnvironmentVariables["PYTHONUTF8"] = "1";
                start.EnvironmentVariables["PYTHONDONTWRITEBYTECODE"] = "1";
                using (Process p = Process.Start(start)) { p.WaitForExit(); return p.ExitCode; }
            } catch (Exception e) {
                MessageBox.Show(e.Message, "VietKara Studio", MessageBoxButtons.OK, MessageBoxIcon.Error); return 1;
            } finally { mutex.ReleaseMutex(); }
        }
    }
}
