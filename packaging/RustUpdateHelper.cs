using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using System.Windows.Forms;

internal sealed class RustUpdate
{
    internal string Version;
    internal string SetupName;
    internal long Size;
    internal string Sha256;
    internal List<UpdatePart> Parts;
}

internal sealed class UpdatePart
{
    internal string Name;
    internal long Size;
    internal string Sha256;
    internal string Url;
}

internal static class RustUpdateHelper
{
    private const string ReleaseApi = "https://api.github.com/repos/NDCLI/VietKaraStudio/releases?per_page=30";
    private const long MaxPartBytes = 1800L * 1024 * 1024;
    private const long MaxSetupBytes = 6L * 1024 * 1024 * 1024;
    private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = 8 * 1024 * 1024 };

    [STAThread]
    private static int Main()
    {
        ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12;
        string root = AppDomain.CurrentDomain.BaseDirectory;
        try
        {
            if (!File.Exists(Path.Combine(root, "installed.flag")))
                throw new InvalidOperationException("Tự cập nhật chỉ hỗ trợ bản Setup đã cài. Bản portable cần tải gói ZIP mới.");
            string current = ReadInstalledVersion(root);
            Dictionary<string, object> release = ReadRelease();
            RustUpdate update = ReadUpdate(release, current);
            if (update == null)
            {
                MessageBox.Show("Bạn đang dùng phiên bản Rust mới nhất (" + current + ").",
                    "Cập nhật VietKara Studio", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return 0;
            }
            string size = (update.Size / (1024.0 * 1024.0 * 1024.0)).ToString("0.00", CultureInfo.InvariantCulture);
            DialogResult answer = MessageBox.Show(
                "Có phiên bản " + update.Version + " (" + size + " GiB).\r\n\r\n" +
                "Tải bộ cài và xác minh SHA-256 trước khi mở? Project, model và cache trong app-data được giữ nguyên.",
                "Cập nhật VietKara Studio", MessageBoxButtons.YesNo, MessageBoxIcon.Question);
            if (answer != DialogResult.Yes) return 0;

            using (DownloadForm form = new DownloadForm(root, update)) form.ShowDialog();
            return 0;
        }
        catch (Exception error)
        {
            MessageBox.Show("Không thể kiểm tra/cập nhật VietKara Studio. Bản đang cài chưa bị thay đổi.\r\n\r\n" +
                error.Message, "Cập nhật VietKara Studio", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }

    private static string ReadInstalledVersion(string root)
    {
        string path = Path.Combine(root, "BUILD-INFO.json");
        Dictionary<string, object> info = Json.DeserializeObject(File.ReadAllText(path, Encoding.UTF8))
            as Dictionary<string, object>;
        string version = info != null && info.ContainsKey("version") ? Convert.ToString(info["version"]) : null;
        if (!ValidVersion(version)) throw new InvalidOperationException("BUILD-INFO.json không có phiên bản hợp lệ.");
        return version;
    }

    private static Dictionary<string, object> ReadRelease()
    {
        IList releases = Json.DeserializeObject(Encoding.UTF8.GetString(
            DownloadMetadata(ReleaseApi, 4 * 1024 * 1024))) as IList;
        if (releases == null) throw new InvalidOperationException("Danh sách GitHub Releases không hợp lệ.");
        foreach (object item in releases)
        {
            Dictionary<string, object> release = item as Dictionary<string, object>;
            if (release == null || AsBool(release, "draft") || AsBool(release, "prerelease")) continue;
            string tag = Convert.ToString(release.ContainsKey("tag_name") ? release["tag_name"] : "");
            string version = tag.StartsWith("v", StringComparison.OrdinalIgnoreCase) ? tag.Substring(1) : tag;
            if (!ValidVersion(version)) continue;
            object rawAssets;
            IList assets = release.TryGetValue("assets", out rawAssets) ? rawAssets as IList : null;
            if (assets == null) continue;
            string setupName = "VietKaraStudio-Rust-" + version + "-Setup-x64.exe";
            bool hasManifest = false;
            bool hasSetupPart = false;
            foreach (object assetValue in assets)
            {
                Dictionary<string, object> asset = assetValue as Dictionary<string, object>;
                if (asset == null) continue;
                string name = Convert.ToString(asset.ContainsKey("name") ? asset["name"] : "");
                if (name == "downloads.json") hasManifest = true;
                if (name == setupName + ".001") hasSetupPart = true;
            }
            if (hasManifest && hasSetupPart) return release;
        }
        throw new InvalidOperationException("Chưa tìm thấy bản Rust stable có đủ asset cập nhật trên GitHub Releases.");
    }

    private static RustUpdate ReadUpdate(Dictionary<string, object> release, string current)
    {
        string tag = Convert.ToString(release.ContainsKey("tag_name") ? release["tag_name"] : "");
        string version = tag.StartsWith("v", StringComparison.OrdinalIgnoreCase) ? tag.Substring(1) : tag;
        if (!ValidVersion(version)) throw new InvalidOperationException("Tag phát hành không đúng định dạng.");
        if (CompareVersions(version, current) <= 0) return null;

        List<Dictionary<string, object>> assets = new List<Dictionary<string, object>>();
        object rawAssets;
        IList assetList = release.TryGetValue("assets", out rawAssets) ? rawAssets as IList : null;
        if (assetList == null)
            throw new InvalidOperationException("Release không có danh sách asset.");
        foreach (object item in assetList)
            if (item is Dictionary<string, object>) assets.Add((Dictionary<string, object>)item);

        string setupName = "VietKaraStudio-Rust-" + version + "-Setup-x64.exe";
        Dictionary<string, object> manifestAsset = assets.Find(item => String.Equals(
            Convert.ToString(item.ContainsKey("name") ? item["name"] : ""), "downloads.json", StringComparison.Ordinal));
        if (manifestAsset == null) throw new InvalidOperationException("Release chưa có downloads.json.");
        byte[] manifestBytes = DownloadMetadata(AssetUrl(manifestAsset), 4 * 1024 * 1024);
        object rawManifest = Json.DeserializeObject(Encoding.UTF8.GetString(manifestBytes));
        IList manifestList = rawManifest as IList;
        if (manifestList == null) throw new InvalidOperationException("downloads.json không hợp lệ.");
        Dictionary<string, object> setup = null;
        foreach (object item in manifestList)
        {
            Dictionary<string, object> candidate = item as Dictionary<string, object>;
            if (candidate != null && String.Equals(Convert.ToString(candidate["name"]), setupName, StringComparison.Ordinal))
            { setup = candidate; break; }
        }
        if (setup == null) throw new InvalidOperationException("Release chưa có bộ cài Rust đúng phiên bản.");

        RustUpdate update = new RustUpdate();
        update.Version = version;
        update.SetupName = setupName;
        update.Size = AsLong(setup, "size");
        update.Sha256 = AsHash(setup, "sha256");
        if (update.Size <= 0 || update.Size > MaxSetupBytes) throw new InvalidOperationException("Kích thước bộ cài không hợp lệ.");
        object rawParts;
        IList partList = setup.TryGetValue("parts", out rawParts) ? rawParts as IList : null;
        if (partList == null || partList.Count < 1 || partList.Count > 10)
            throw new InvalidOperationException("Danh sách phần tải không hợp lệ.");
        update.Parts = new List<UpdatePart>();
        int index = 1;
        long total = 0;
        foreach (object value in partList)
        {
            Dictionary<string, object> spec = value as Dictionary<string, object>;
            if (spec == null) throw new InvalidOperationException("Thông tin phần tải không hợp lệ.");
            string expectedName = setupName + "." + index.ToString("000", CultureInfo.InvariantCulture);
            string name = Convert.ToString(spec.ContainsKey("name") ? spec["name"] : "");
            long partSize = AsLong(spec, "size");
            if (!String.Equals(name, expectedName, StringComparison.Ordinal) || partSize <= 0 || partSize > MaxPartBytes)
                throw new InvalidOperationException("Tên/kích thước phần tải không khớp.");
            Dictionary<string, object> asset = assets.Find(item => String.Equals(
                Convert.ToString(item.ContainsKey("name") ? item["name"] : ""), name, StringComparison.Ordinal));
            if (asset == null || AsLong(asset, "size") != partSize)
                throw new InvalidOperationException("Thiếu asset hoặc kích thước asset GitHub không khớp: " + name);
            update.Parts.Add(new UpdatePart { Name = name, Size = partSize,
                Sha256 = AsHash(spec, "sha256"), Url = AssetUrl(asset) });
            total += partSize;
            index++;
        }
        if (total != update.Size) throw new InvalidOperationException("Tổng dung lượng các phần không khớp manifest.");
        return update;
    }

    private static Dictionary<string, object> ReadJson(string url, int maxBytes)
    {
        byte[] bytes = DownloadMetadata(url, maxBytes);
        Dictionary<string, object> value = Json.DeserializeObject(Encoding.UTF8.GetString(bytes)) as Dictionary<string, object>;
        if (value == null) throw new InvalidOperationException("JSON từ GitHub không hợp lệ.");
        return value;
    }

    private static byte[] DownloadMetadata(string url, int maxBytes)
    {
        HttpWebRequest request = CreateRequest(url, true);
        using (HttpWebResponse response = (HttpWebResponse)request.GetResponse())
        {
            ValidateResponseHost(response.ResponseUri.Host, url.Contains("api.github.com"));
            using (Stream input = response.GetResponseStream())
            using (MemoryStream output = new MemoryStream())
            {
                byte[] buffer = new byte[32768];
                int read;
                while ((read = input.Read(buffer, 0, buffer.Length)) > 0)
                {
                    if (output.Length + read > maxBytes) throw new InvalidOperationException("Metadata GitHub vượt giới hạn dung lượng.");
                    output.Write(buffer, 0, read);
                }
                return output.ToArray();
            }
        }
    }

    private static HttpWebRequest CreateRequest(string url, bool json)
    {
        Uri uri;
        if (!Uri.TryCreate(url, UriKind.Absolute, out uri) || uri.Scheme != Uri.UriSchemeHttps ||
            !(uri.Host == "api.github.com" || uri.Host == "github.com" || uri.Host.EndsWith(".githubusercontent.com", StringComparison.OrdinalIgnoreCase)))
            throw new InvalidOperationException("URL tải không thuộc GitHub qua HTTPS.");
        HttpWebRequest request = (HttpWebRequest)WebRequest.Create(uri);
        request.UserAgent = "VietKaraStudio-Rust-Updater";
        request.Accept = json ? "application/vnd.github+json" : "application/octet-stream";
        request.Timeout = 30000;
        request.ReadWriteTimeout = 60000;
        request.AllowAutoRedirect = true;
        return request;
    }

    private static string AssetUrl(Dictionary<string, object> asset)
    {
        string url = Convert.ToString(asset.ContainsKey("browser_download_url") ? asset["browser_download_url"] : "");
        Uri parsed;
        if (!Uri.TryCreate(url, UriKind.Absolute, out parsed) || parsed.Scheme != Uri.UriSchemeHttps || parsed.Host != "github.com")
            throw new InvalidOperationException("URL asset GitHub không hợp lệ.");
        return url;
    }

    private static void ValidateResponseHost(string host, bool api)
    {
        bool valid = api ? host == "api.github.com" :
            host == "github.com" || host.EndsWith(".githubusercontent.com", StringComparison.OrdinalIgnoreCase);
        if (!valid) throw new InvalidOperationException("Redirect tải file rời khỏi GitHub.");
    }

    private static string DownloadAndVerify(string url, string target, long expectedSize, string expectedHash,
        long baseBytes, long totalBytes, BackgroundWorker worker)
    {
        if (File.Exists(target))
        {
            FileInfo cached = new FileInfo(target);
            if (cached.Length == expectedSize && HashFile(target) == expectedHash) return target;
            throw new IOException("Tệp tải đã tồn tại nhưng SHA-256 sai; hãy bỏ tệp lỗi rồi thử lại.");
        }
        string partial = target + ".partial";
        if (File.Exists(partial)) throw new IOException("Đã có tệp tải dở. Hãy xóa tệp .partial rồi thử lại.");
        HttpWebRequest request = CreateRequest(url, false);
        using (HttpWebResponse response = (HttpWebResponse)request.GetResponse())
        {
            ValidateResponseHost(response.ResponseUri.Host, false);
            using (Stream input = response.GetResponseStream())
            using (FileStream output = new FileStream(partial, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            using (SHA256 hash = SHA256.Create())
            {
                byte[] buffer = new byte[1024 * 1024];
                long written = 0;
                int read;
                while ((read = input.Read(buffer, 0, buffer.Length)) > 0)
                {
                    written += read;
                    if (written > expectedSize) throw new IOException("Asset tải vượt kích thước manifest.");
                    output.Write(buffer, 0, read);
                    hash.TransformBlock(buffer, 0, read, buffer, 0);
                    int percent = (int)Math.Min(99, ((baseBytes + written) * 100L) / totalBytes);
                    worker.ReportProgress(percent, "Đang tải " + Path.GetFileName(target) + " · " +
                        (written / (1024 * 1024)).ToString("N0") + " / " + (expectedSize / (1024 * 1024)).ToString("N0") + " MiB");
                }
                hash.TransformFinalBlock(new byte[0], 0, 0);
                output.Flush(true);
                if (written != expectedSize || ToHex(hash.Hash) != expectedHash)
                    throw new IOException("Dung lượng hoặc SHA-256 của asset không khớp.");
            }
        }
        File.Move(partial, target);
        return target;
    }

    private static void DownloadAndLaunch(string root, RustUpdate update, BackgroundWorker worker)
    {
        string appData = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "VietKaraStudio-Rust");
        string folder = Path.Combine(appData, "updates", update.Version);
        Directory.CreateDirectory(folder);
        string final = Path.Combine(folder, update.SetupName);
        if (File.Exists(final))
        {
            if (new FileInfo(final).Length != update.Size || HashFile(final) != update.Sha256)
                throw new IOException("Bộ cài đã tải có SHA-256 sai; hãy bỏ tệp lỗi rồi thử lại.");
        }
        else
        {
            string volume = Path.GetPathRoot(folder);
            if (new DriveInfo(volume).AvailableFreeSpace < update.Size * 2 + 256L * 1024 * 1024)
                throw new IOException("Không đủ dung lượng trống để tải và xác minh bộ cài.");
            long downloaded = 0;
            List<string> parts = new List<string>();
            foreach (UpdatePart part in update.Parts)
            {
                string path = Path.Combine(folder, part.Name);
                DownloadAndVerify(part.Url, path, part.Size, part.Sha256, downloaded, update.Size, worker);
                parts.Add(path);
                downloaded += part.Size;
            }
            string partial = final + ".partial";
            if (File.Exists(partial)) throw new IOException("Đã có bộ cài ghép dở; hãy bỏ tệp .partial rồi thử lại.");
            using (FileStream output = new FileStream(partial, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            using (SHA256 hash = SHA256.Create())
            {
                byte[] buffer = new byte[1024 * 1024];
                long written = 0;
                foreach (string part in parts)
                using (FileStream input = File.OpenRead(part))
                {
                    int read;
                    while ((read = input.Read(buffer, 0, buffer.Length)) > 0)
                    {
                        output.Write(buffer, 0, read);
                        hash.TransformBlock(buffer, 0, read, buffer, 0);
                        written += read;
                    }
                }
                hash.TransformFinalBlock(new byte[0], 0, 0);
                output.Flush(true);
                if (written != update.Size || ToHex(hash.Hash) != update.Sha256)
                    throw new IOException("SHA-256 của bộ cài đã ghép không khớp.");
            }
            File.Move(partial, final);
            foreach (string part in parts)
                try { File.Delete(part); } catch { }
        }
        worker.ReportProgress(100, "Đã xác minh bộ cài. Đang mở Setup…");
        using (Process installer = Process.Start(new ProcessStartInfo {
            FileName = final, WorkingDirectory = folder, UseShellExecute = true }))
        {
            if (installer == null) throw new InvalidOperationException("Không mở được bộ cài.");
            installer.WaitForExit();
            if (installer.ExitCode == 0)
                try { File.Delete(final); } catch { }
        }
    }

    private static string HashFile(string path)
    {
        using (FileStream input = File.OpenRead(path))
        using (SHA256 hash = SHA256.Create()) return ToHex(hash.ComputeHash(input));
    }

    private static string ToHex(byte[] bytes)
    {
        StringBuilder result = new StringBuilder(bytes.Length * 2);
        foreach (byte value in bytes) result.Append(value.ToString("x2", CultureInfo.InvariantCulture));
        return result.ToString();
    }

    private static string AsHash(Dictionary<string, object> value, string key)
    {
        string hash = Convert.ToString(value.ContainsKey(key) ? value[key] : "");
        if (!Regex.IsMatch(hash, "^[0-9a-fA-F]{64}$")) throw new InvalidOperationException("SHA-256 trong manifest không hợp lệ.");
        return hash.ToLowerInvariant();
    }

    private static long AsLong(Dictionary<string, object> value, string key)
    {
        object number;
        long result;
        if (!value.TryGetValue(key, out number) || !Int64.TryParse(Convert.ToString(number, CultureInfo.InvariantCulture), out result))
            throw new InvalidOperationException("Kích thước trong manifest không hợp lệ.");
        return result;
    }

    private static bool AsBool(Dictionary<string, object> value, string key)
    { return value.ContainsKey(key) && value[key] is bool && (bool)value[key]; }

    private static bool ValidVersion(string value)
    { return !String.IsNullOrEmpty(value) && Regex.IsMatch(value, "^\\d+\\.\\d+\\.\\d+$"); }

    private static int CompareVersions(string left, string right)
    {
        Version a, b;
        if (!Version.TryParse(left, out a) || !Version.TryParse(right, out b))
            throw new InvalidOperationException("Không so sánh được phiên bản.");
        return a.CompareTo(b);
    }

    private sealed class DownloadForm : Form
    {
        private readonly string root;
        private readonly RustUpdate update;
        private readonly Label message = new Label();
        private readonly ProgressBar progress = new ProgressBar();
        private readonly Button close = new Button();
        private readonly BackgroundWorker worker = new BackgroundWorker();
        private bool finished;

        internal DownloadForm(string appRoot, RustUpdate selected)
        {
            root = appRoot;
            update = selected;
            Text = "Cập nhật VietKara Studio";
            Width = 520;
            Height = 150;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;
            StartPosition = FormStartPosition.CenterScreen;
            message.SetBounds(16, 16, 475, 30);
            message.Text = "Đang chuẩn bị tải bản " + update.Version + "…";
            progress.SetBounds(16, 52, 475, 22);
            progress.Minimum = 0;
            progress.Maximum = 100;
            close.SetBounds(391, 82, 100, 28);
            close.Text = "Đóng";
            close.Enabled = false;
            close.Click += delegate { Close(); };
            Controls.Add(message);
            Controls.Add(progress);
            Controls.Add(close);
            worker.WorkerReportsProgress = true;
            worker.DoWork += delegate(object sender, DoWorkEventArgs args)
            { DownloadAndLaunch(root, update, worker); };
            worker.ProgressChanged += delegate(object sender, ProgressChangedEventArgs args)
            {
                progress.Value = Math.Max(0, Math.Min(100, args.ProgressPercentage));
                message.Text = Convert.ToString(args.UserState);
            };
            worker.RunWorkerCompleted += delegate(object sender, RunWorkerCompletedEventArgs args)
            {
                finished = true;
                close.Enabled = true;
                if (args.Error != null)
                {
                    message.Text = "Không tải/cài được bản cập nhật.";
                    MessageBox.Show(this, args.Error.GetBaseException().Message,
                        "Cập nhật thất bại", MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
                else message.Text = "Đã cài xong. Studio sẽ mở lại sau khi cập nhật.";
            };
            Shown += delegate { worker.RunWorkerAsync(); };
            FormClosing += delegate(object sender, FormClosingEventArgs args)
            { if (!finished) args.Cancel = true; };
        }
    }
}
