$ErrorActionPreference = 'Stop'

$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $compiler)) {
    throw "C# Framework compiler not found: $compiler"
}

$tempRoot = [System.IO.Path]::GetFullPath((Join-Path $env:TEMP ('vietkara-update-helper-' + [guid]::NewGuid().ToString('N'))))
$tempBase = [System.IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (-not $tempRoot.StartsWith($tempBase, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Test directory resolved outside TEMP.'
}
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    $harnessPath = Join-Path $tempRoot 'UpdaterSmoke.cs'
    $exePath = Join-Path $tempRoot 'UpdaterSmoke.exe'
    @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;

internal static class RustUpdateHelperSmoke
{
    private sealed class InterruptedStream : MemoryStream
    {
        private readonly int limit;
        internal InterruptedStream(byte[] data, int readLimit) : base(data) { limit = readLimit; }
        public override int Read(byte[] buffer, int offset, int count)
        {
            if (Position >= limit) throw new IOException("synthetic network interruption");
            return base.Read(buffer, offset, Math.Min(count, limit - (int)Position));
        }
    }

    private static string Hash(byte[] value)
    {
        using (SHA256 hash = SHA256.Create())
        {
            StringBuilder result = new StringBuilder();
            foreach (byte item in hash.ComputeHash(value)) result.Append(item.ToString("x2"));
            return result.ToString();
        }
    }

    private static void Require(bool condition, string message)
    { if (!condition) throw new Exception(message); }

    private static void Invoke(MethodInfo method, Stream stream, string target, long size, string digest,
        BackgroundWorker worker = null)
    {
        method.Invoke(null, new object[] { stream, target, size, digest, 0L, Math.Max(1L, size), worker });
    }

    private static void RequireFailure(MethodInfo method, Stream stream, string target, long size, string digest)
    {
        bool failed = false;
        try { Invoke(method, stream, target, size, digest); }
        catch (TargetInvocationException) { failed = true; }
        Require(failed, "Expected updater verification to fail.");
        Require(!File.Exists(target), "Failed download published a destination file.");
        Require(!File.Exists(target + ".partial"), "Failed download left an owned partial file.");
    }

    private static void RequireJoinFailure(MethodInfo method, List<string> parts, string target,
        long size, string digest)
    {
        bool failed = false;
        try { method.Invoke(null, new object[] { parts, target, size, digest, null }); }
        catch (TargetInvocationException) { failed = true; }
        Require(failed, "Expected updater part joining to fail.");
        Require(!File.Exists(target), "Failed join published a destination file.");
        Require(!File.Exists(target + ".partial"), "Failed join left an owned partial file.");
    }

    internal static int Main(string[] args)
    {
        string root = args[0];
        byte[] data = Encoding.UTF8.GetBytes("VietKara updater integrity smoke");
        Assembly assembly = Assembly.GetExecutingAssembly();
        Type helper = assembly.GetType("RustUpdateHelper", true);
        MethodInfo save = helper.GetMethod("SaveAndVerifyStream", BindingFlags.Static | BindingFlags.NonPublic);
        MethodInfo join = helper.GetMethod("JoinAndVerifyParts", BindingFlags.Static | BindingFlags.NonPublic);
        MethodInfo installerExit = helper.GetMethod("EnsureInstallerSucceeded", BindingFlags.Static | BindingFlags.NonPublic);
        Require(save != null, "SaveAndVerifyStream not found.");
        Require(join != null, "JoinAndVerifyParts not found.");
        Require(installerExit != null, "EnsureInstallerSucceeded not found.");
        installerExit.Invoke(null, new object[] { 0, "verified-setup.exe" });
        bool installerFailureReported = false;
        try { installerExit.Invoke(null, new object[] { 7, "verified-setup.exe" }); }
        catch (TargetInvocationException error)
        {
            installerFailureReported = error.InnerException is InvalidOperationException &&
                error.InnerException.Message.Contains("verified-setup.exe");
        }
        Require(installerFailureReported, "Non-zero Setup exit code was reported as success.");

        string successPath = Path.Combine(root, "success.bin");
        Invoke(save, new MemoryStream(data), successPath, data.Length, Hash(data));
        Require(File.Exists(successPath) && File.ReadAllBytes(successPath).Length == data.Length,
            "Valid asset was not published.");

        string badHashPath = Path.Combine(root, "bad-hash.bin");
        RequireFailure(save, new MemoryStream(data), badHashPath, data.Length, new string('0', 64));

        string interruptedPath = Path.Combine(root, "interrupted.bin");
        RequireFailure(save, new InterruptedStream(data, 7), interruptedPath, data.Length, Hash(data));

        string occupiedPath = Path.Combine(root, "occupied.bin");
        File.WriteAllText(occupiedPath + ".partial", "leave this file untouched");
        bool collisionFailed = false;
        try { Invoke(save, new MemoryStream(data), occupiedPath, data.Length, Hash(data)); }
        catch (TargetInvocationException) { collisionFailed = true; }
        Require(collisionFailed, "Existing partial-file collision was not rejected.");
        Require(File.ReadAllText(occupiedPath + ".partial") == "leave this file untouched",
            "Updater removed a partial file it did not create.");

        string cancelledPath = Path.Combine(root, "cancelled.bin");
        using (BackgroundWorker cancellationWorker = new BackgroundWorker())
        using (System.Threading.ManualResetEvent completed = new System.Threading.ManualResetEvent(false))
        {
            cancellationWorker.WorkerSupportsCancellation = true;
            bool cancellationObserved = false;
            cancellationWorker.DoWork += delegate(object sender, DoWorkEventArgs workArgs)
            {
                cancellationWorker.CancelAsync();
                try { Invoke(save, new MemoryStream(data), cancelledPath, data.Length, Hash(data), cancellationWorker); }
                catch (TargetInvocationException error)
                {
                    cancellationObserved = error.InnerException is OperationCanceledException;
                    workArgs.Cancel = cancellationObserved;
                }
            };
            cancellationWorker.RunWorkerCompleted += delegate { completed.Set(); };
            cancellationWorker.RunWorkerAsync();
            Require(completed.WaitOne(TimeSpan.FromSeconds(10)), "Updater cancellation smoke timed out.");
            Require(cancellationObserved, "Updater did not observe the cancellation request.");
            Require(!File.Exists(cancelledPath) && !File.Exists(cancelledPath + ".partial"),
                "Cancelled download published a destination or left its partial file.");
        }

        int split = data.Length / 2;
        string partOne = Path.Combine(root, "release.001");
        string partTwo = Path.Combine(root, "release.002");
        File.WriteAllBytes(partOne, new byte[split]);
        File.WriteAllBytes(partTwo, new byte[data.Length - split]);
        using (FileStream output = File.Create(partOne)) output.Write(data, 0, split);
        using (FileStream output = File.Create(partTwo)) output.Write(data, split, data.Length - split);
        List<string> parts = new List<string> { partOne, partTwo };

        string joinedPath = Path.Combine(root, "joined.exe");
        join.Invoke(null, new object[] { parts, joinedPath, (long)data.Length, Hash(data), null });
        Require(File.Exists(joinedPath) && Hash(File.ReadAllBytes(joinedPath)) == Hash(data),
            "Valid release parts were not joined correctly.");

        string badJoinPath = Path.Combine(root, "bad-join.exe");
        RequireJoinFailure(join, parts, badJoinPath, data.Length, new string('0', 64));

        string missingPartPath = Path.Combine(root, "missing-part.exe");
        RequireJoinFailure(join, new List<string> { partOne, Path.Combine(root, "missing.003") },
            missingPartPath, data.Length, Hash(data));

        string occupiedJoinPath = Path.Combine(root, "occupied-join.exe");
        File.WriteAllText(occupiedJoinPath + ".partial", "leave this join file untouched");
        bool joinCollisionFailed = false;
        try { join.Invoke(null, new object[] { parts, occupiedJoinPath, (long)data.Length, Hash(data), null }); }
        catch (TargetInvocationException) { joinCollisionFailed = true; }
        Require(joinCollisionFailed, "Existing join partial-file collision was not rejected.");
        Require(File.ReadAllText(occupiedJoinPath + ".partial") == "leave this join file untouched",
            "Updater removed a join partial file it did not create.");

        Console.WriteLine("Updater integrity/cancellation smoke passed (download, cancellation cleanup, Setup exit codes, and part joining).");
        return 0;
    }
}
'@ | Set-Content -LiteralPath $harnessPath -Encoding ASCII

    $source = (Resolve-Path (Join-Path $PSScriptRoot '..\packaging\RustUpdateHelper.cs')).Path
    & $compiler /nologo /target:exe /main:RustUpdateHelperSmoke /optimize+ `
        /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll `
        "/out:$exePath" $source $harnessPath
    if ($LASTEXITCODE -ne 0) { throw "C# smoke harness compile failed: $LASTEXITCODE" }

    & $exePath $tempRoot
    if ($LASTEXITCODE -ne 0) { throw "Updater integrity smoke failed: $LASTEXITCODE" }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
