using System.Diagnostics;
using System.Runtime.CompilerServices;

namespace StarlightDocNet;

internal class Program
{
    private static readonly Lock logListLock = new();

    static void Main(string[] args)
    {
        Assert(args.Length >= 2, "Expected usage: dotnet run StarlightDocNet path/to/api_yml/ path/to/dst/md/");

        string srcPath = args[0];
        string dstPath = args[1];

        Stopwatch sw = Stopwatch.StartNew();
        Log("Preparing to process files...");
        var srcFiles = Directory.EnumerateFiles(srcPath).ToArray();

        var apiDocRootInd = dstPath.LastIndexOf("docs");
        var apiDocRoot = "/";
        if (apiDocRootInd != -1)
            apiDocRoot = dstPath[(apiDocRootInd + 4)..].Replace(Path.DirectorySeparatorChar, '/');
        if (!apiDocRoot.EndsWith('/'))
            apiDocRoot += '/';

        var srcFileNames = srcFiles.Select(x => Path.GetFileNameWithoutExtension(x)).ToHashSet();

        Parallel.ForEach(srcFiles, path =>
        {
            YML2MD.Process(path, dstPath, apiDocRoot, srcFileNames);
        });
        //foreach (var path in srcFiles)
        //    YML2MD.Process(path, dstPath, dstPath, srcFileNames);
        sw.Stop();
        Log($"Completed in {sw}!");
    }

    public static void Assert(bool condition, string? msg = null, [CallerArgumentExpression(nameof(condition))] string? expr = null, [CallerMemberName] string caller = "")
    {
        if (condition)
            return;

        if (msg != null)
            Log(msg, LogLevel.Error, caller);
        else
            Log($"Assertion failed: {expr}", LogLevel.Error, caller);

#if DEBUG
        Debugger.Break();
#endif
        Environment.Exit(-1);
    }

    public static void Log(object message, LogLevel level = LogLevel.Info, [CallerMemberName] string caller = "")
    {
#if !DEBUG
        if (level <= LogLevel.Debug)
            return;
#endif

        lock (logListLock)
        {
            var time = DateTime.Now;
            string messageString = message?.ToString() ?? "null";
            string msg = $"[{level}] [{time}] [{caller}] {messageString}";
            Console.WriteLine(msg);
            Debug.WriteLine(msg);
        }
    }

    public enum LogLevel
    {
        Debug,
        Info,
        Warning,
        Error
    }
}
