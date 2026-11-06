using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using System.Windows.Forms;

[assembly: AssemblyTitle("VietKara Studio Rust")]
[assembly: AssemblyProduct("VietKara Studio Rust")]
[assembly: AssemblyDescription("Native karaoke studio")]
[assembly: AssemblyVersion("0.1.0.0")]
[assembly: AssemblyFileVersion("0.1.0.0")]

internal static class RustLauncher
{
    private static string Quote(string value)
    {
        StringBuilder result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char character in value)
        {
            if (character == '\\') { slashes++; continue; }
            if (character == '"')
            {
                result.Append('\\', slashes * 2 + 1).Append('"');
                slashes = 0;
                continue;
            }
            result.Append('\\', slashes).Append(character);
            slashes = 0;
        }
        result.Append('\\', slashes * 2).Append('"');
        return result.ToString();
    }

    [STAThread]
    private static int Main(string[] args)
    {
        bool created;
        using (Mutex mutex = new Mutex(true, "Local\\VietKaraStudio_Rust", out created))
        {
            if (!created)
            {
                MessageBox.Show("VietKara Studio đang chạy. Hãy dùng cửa sổ hiện có.",
                    "VietKara Studio", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return 0;
            }

            try
            {
                string root = AppDomain.CurrentDomain.BaseDirectory;
                string data = Path.Combine(Environment.GetFolderPath(
                    Environment.SpecialFolder.LocalApplicationData), "VietKaraStudio-Rust");
                Directory.CreateDirectory(data);

                ProcessStartInfo start = new ProcessStartInfo
                {
                    FileName = Path.Combine(root, "app", "vietkara-app.exe"),
                    WorkingDirectory = root,
                    UseShellExecute = false,
                    CreateNoWindow = true
                };
                StringBuilder arguments = new StringBuilder();
                foreach (string argument in args)
                {
                    if (arguments.Length > 0) arguments.Append(' ');
                    arguments.Append(Quote(argument));
                }
                start.Arguments = arguments.ToString();
                start.EnvironmentVariables["OBS_DATA_DIR"] = data;
                start.EnvironmentVariables["VIETKARA_PYTHON"] = Path.Combine(root, "runtime", "python.exe");
                start.EnvironmentVariables["VIETKARA_AI_WORKER"] = Path.Combine(root, "worker", "ai_worker.py");
                start.EnvironmentVariables["OBS_FFMPEG"] = Path.Combine(root, "app", "ffmpeg.exe");
                start.EnvironmentVariables["OBS_FFPROBE"] = Path.Combine(root, "app", "ffprobe.exe");
                start.EnvironmentVariables["PYTHONUTF8"] = "1";
                start.EnvironmentVariables["PYTHONDONTWRITEBYTECODE"] = "1";

                using (Process app = Process.Start(start))
                {
                    if (app == null) throw new InvalidOperationException("Không khởi chạy được VietKara Studio.");
                    app.WaitForExit();
                    return app.ExitCode;
                }
            }
            catch (Exception error)
            {
                MessageBox.Show(error.Message, "Không mở được VietKara Studio",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
            finally
            {
                if (created) mutex.ReleaseMutex();
            }
        }
    }
}
