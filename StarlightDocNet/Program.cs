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
        foreach (var path in Directory.EnumerateFiles(srcPath))
        {
            YML2MD yml2md = new();
            yml2md.Process(path, dstPath);
        }
        sw.Stop();
        Log($"Completed it {sw}!");
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
