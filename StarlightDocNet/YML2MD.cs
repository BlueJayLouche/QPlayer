using Docfx.Build.ApiPage;
using System;
using System.Buffers;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text;
using YamlDotNet.Core;
using YamlDotNet.Serialization;
using static StarlightDocNet.Program;

namespace StarlightDocNet;

internal static class YML2MD
{
    public static void Process(string srcPath, string dstDir, string apiDocRoot, HashSet<string> srcs)
    {
        // File path stuff
        var fname = Path.GetFileNameWithoutExtension(srcPath);
        int prefixInd = fname.LastIndexOf('.');
        var subdir = prefixInd != -1 ? fname[..prefixInd] : string.Empty;
        var shortName = prefixInd != -1 ? fname[(prefixInd + 1)..] : fname;
        subdir = subdir.Replace('.', Path.DirectorySeparatorChar);
        var dstPath = Path.Combine(dstDir, subdir, shortName + ".md");

        if (fname == "toc")
            return;
        Log($"Processing {fname}...");

        using var tr = File.OpenText(srcPath);
        var parser = new Parser(tr);
        var ser = new Deserializer();

        var doc = ser.Deserialize(tr) as IDictionary<object, object>;
        Assert(doc != null);

        var md = new MDStringBuilder(apiDocRoot, srcs);

        var title = (string)doc!["title"];
        var titleSplit = title.IndexOf(' ');
        var docType = title.AsSpan(0, titleSplit) switch
        {
            "Class" => DocType.Class,
            "Namespace" => DocType.Namespace,
            "Enum" => DocType.Enum,
            "Struct" => DocType.Struct,
            "Interface" => DocType.Interface,
            "Delegate" => DocType.Delegate,
            _ => throw new Exception()
        };

        md.MetaHeader(title, null, docType == DocType.Namespace ? 0 : 10, title[(titleSplit + 1)..]);

        var body = doc["body"].AsArray<IDictionary<object, object>>();
        int i = 0;
        var apiDoc = body[i++];
        var api = apiDoc.As<Api1>();
        md.H1("Definition");
        if (api.src != null)
            md.SrcLink(api.src);

        if (body[i].ContainsKey("facts"))
        {
            var facts = body[1].As<Facts>();
            foreach (var fact in facts.facts)
                md.Italic(fact.name).Add(": ").Add(fact.value).Break();
            md.Para();
            i++;
        }

        if (body[i].ContainsKey("code"))
        {
            md.CodeBlock(body[i].As<Code>());
            i++;
        }

        bool injectSummary = true;
        for (; i < body.Length; i++)
        {
            var item = body[i];
            foreach (var key in item.Keys)
            {
                switch (key)
                {
                    case "h1": md.H1((string)item["h1"]); break;
                    case "h2":
                        if (injectSummary)
                        {
                            injectSummary = false;
                            CreateSummary(md, body, i);
                        }
                        md.H2((string)item["h2"]);
                        break;
                    case "h3": md.H3((string)item["h3"]); break;
                    case "h4": md.H4((string)item["h4"]); break;

                    case "markdown": md.Add(item.As<Markdown>()); break;
                    case "inheritance": md.Add(item.As<Inheritance>()); break;
                    case "list": md.Add(item.As<List>()); break;
                    case "api2": md.Add(item.As<Api2>()); break;
                    case "api3": md.Add(item.As<Api3>()); break;
                    case "api4": md.Add(item.As<Api4>()); break;
                    case "code": md.CodeBlock(item.As<Code>()); break;
                    case "parameters": md.Add(item.As<Parameters>()); break;

                    default: break;
                }
            }
        }

        var dstFinalDir = Path.GetDirectoryName(dstPath)!;
        if (!Directory.Exists(dstFinalDir))
            Directory.CreateDirectory(dstFinalDir);
        File.WriteAllText(dstPath, md.ToString());
    }

    private static void CreateSummary(MDStringBuilder md, IDictionary<object, object>[] body, int ind)
    {
        md.H2("Summary");
        using var table = md.Table();
        table.HeaderCell("Type");
        table.HeaderCell("Name");
        table.HeaderCell("Description");
        string nextItemName = string.Empty;
        string? nextDesc = null;
        for (int i = ind; i < body.Length; i++)
        {
            var item = body[i];
            if (item.TryGetValue("h2", out var h2))
            {
                WriteSummaryItem();
                table.BeginCell();
                md.Bold(h2.ToString()?.ToUpperInvariant() ?? string.Empty);
                table.EndCell();
                table.Cell();
                table.Cell();
            }
            else if (item.ContainsKey("api3"))
            {
                WriteSummaryItem();
                var api3 = item.As<Api3>();
                nextItemName = api3.api3;
                nextDesc = null;
            }
            else if (item.TryGetValue("markdown", out var desc))
            {
                nextDesc = desc.ToString()?.Without('\n', '\r');
            }
        }
        WriteSummaryItem();

        void WriteSummaryItem()
        {
            if (nextItemName == string.Empty)
                return;

            var itemSlug = '#' + nextItemName.RemoveTypeParam()
                .Without(',', '<', '>', '(', ')', '[', ']', '@', '#', '^', '`', '?', '~')
                .Replace(' ', '-').ToLowerInvariant();

            table.Cell();
            table.BeginCell();
            md.Link(itemSlug, nextItemName.EscapeLink());
            table.EndCell();
            table.BeginCell();
            if (nextDesc != null)
                md.AddMarkdown(nextDesc);
            table.EndCell();
            nextDesc = null;
            nextItemName = string.Empty;
        }
    }

    enum DocType
    {
        Namespace,
        Class,
        Interface,
        Struct,
        Enum,
        Delegate
    }
}

public static partial class ExtensionMethods
{
    [ThreadStatic]
    private static StringBuilder? sbInst;
    private static StringBuilder SharedStringBuilder => (sbInst ??= new()).Clear();
    internal static string Without(this string str, params Span<char> toRemove)
    {
        var sb = SharedStringBuilder;
        sb.EnsureCapacity(str.Length);

        foreach (var c in str)
            if (!toRemove.Contains(c))
                sb.Append(c);

        return sb.ToString();
    }

    internal static string RemoveTypeParam(this string str)
    {
        var sb = SharedStringBuilder;
        sb.EnsureCapacity(str.Length);

        int tpInd;
        var s = str.AsSpan();
        while ((tpInd = s.IndexOf('<')) != -1)
        {
            sb.Append(s[..tpInd]);
            int end = s.IndexOf('>');
            if (end == -1)
            {
                s = s[(tpInd + 1)..];
                break;
            }
            s = s[(end + 1)..];
        }
        sb.Append(s);

        return sb.ToString();
    }

    internal static string EscapeLink(this string str)
    {
        var sb = SharedStringBuilder;
        sb.EnsureCapacity(str.Length);

        foreach (var c in str)
        {
            if (c == '[' || c == ']')
                sb.Append('\\');
            sb.Append(c);
        }

        return sb.ToString();
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Inline? inline)
    {
        if (inline == null)
            return md;
        if (inline.IsT0)
        {
            md.Add(inline.AsT0);
            return md;
        }
        else
        {
            var arr = inline.AsT1;
            if (arr.Length == 0)
                return md;

            md.Add(arr[0]);
            for (int i = 1; i < arr.Length; i++)
                md/*.Add(", ")*/.Add(arr[i]);

            return md;
        }
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Span span)
    {
        if (span.IsT0)
        {
            md.Add(span.AsT0);
            return md;
        }
        else
        {
            var link = span.AsT1;
            var url = link.url ?? string.Empty;
            if (!url.StartsWith("http"))
            {
                var sb = SharedStringBuilder;
                sb.Append(md.localURLRoot);

                var section = url.LastIndexOf('#');
                var fileExt = url.LastIndexOf(".html");
                if (fileExt != -1)
                    sb.Append(url.AsSpan(0, fileExt));
                else if (section != -1)
                    sb.Append(url.AsSpan(0, section));
                else
                    sb.Append(fileExt);
                sb.Replace('.', '/');

                url = sb.ToString().ToLowerInvariant();
            }
            md.Link(url, link.text);
            return md;
        }
    }

    internal static MDStringBuilder AddMarkdown(this MDStringBuilder md, string? str)
    {
        if (str == null)
            return md;

        var urlSb = SharedStringBuilder;
        var s = str.AsSpan();
        int xrefPos;
        while ((xrefPos = s.IndexOf("<xref")) != -1)
        {
            // Append everything up until the match
            md.Add(s[..xrefPos]);

            // Find the end of the xref
            int end = s.IndexOf("</xref>");
            if (end == -1)
                break; // malformed
            else
                end += 7;

            // Find and convert the href
            int href = s.IndexOf("href=\"");
            if (href == -1)
                break; // malformed
            else
                href += 6;
            int hrefEnd = s[href..].IndexOf('"');
            if (hrefEnd == -1)
                break; //malformed
            else
                hrefEnd += href;

            var hrefSpan = s[href..hrefEnd];
            var hrefStr = new string(hrefSpan);
            hrefStr = hrefStr.Replace("%60", "-");
            // This could look something like: QPlayer.ViewModels.MainViewModel.activeCues
            // we now need to convert this into an actual url
            if (md.knownPages.Contains(hrefStr))
            {
                AppendURL(hrefStr);
            }
            else
            {
                // Try to link to a member on a given page instead
                int bracket = hrefSpan.IndexOfAny('(', '[', '<');
                int dot;
                if (bracket == -1)
                    dot = hrefSpan.LastIndexOf('.');
                else
                    dot = hrefSpan[..bracket].LastIndexOf('.');
                var hrefPrefix = hrefSpan[..dot];
                string hrefPrefStr = new(hrefPrefix);
                string hrefSuffix = new(hrefSpan[(dot + 1)..]);
                hrefPrefStr = hrefPrefStr.Replace("%60", "-");
                hrefSuffix = Uri.UnescapeDataString(hrefSuffix.Replace("%60", "-")); // TODO: this will never correctly resolve for method xrefs as the parameter types always use fully qualified names and never aliases.
                if (md.knownPages.Contains(hrefPrefStr))
                    AppendURL(hrefPrefStr, hrefSuffix);
                else
                    md.Code(hrefSuffix);
            }

            s = s[end..];
        }
        md.Add(s);
        return md;

        void AppendURL(string hrefStr, string? section = null)
        {
            urlSb.Clear();
            urlSb.Append(md.localURLRoot);
            urlSb.Append(hrefStr.ToLowerInvariant());
            urlSb.Replace('.', '/');

            if (section != null)
            {
                urlSb.Append('#');
                urlSb.Append(section.ToLowerInvariant());
            }


            var memberName = section;
            if (memberName == null)
            {
                int bracket = hrefStr.IndexOfAny('(', '[', '<');
                int dot;
                if (bracket == -1)
                    dot = hrefStr.LastIndexOf('.');
                else
                    dot = hrefStr.LastIndexOf('.', 0, bracket);

                if (dot != -1)
                    memberName = hrefStr[(dot + 1)..];
                else
                    memberName = hrefStr;
            }
            md.Link(urlSb.ToString(), section ?? memberName);
        }
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Markdown val) => AddMarkdown(md, val.markdown).Line();

    internal static MDStringBuilder CodeBlock(this MDStringBuilder md, Code code)
    {
        using (md.CodeBlock(code.languageId ?? "cs"))
            md.Line(code.code);
        return md;
    }

    internal static MDStringBuilder Code(this MDStringBuilder md, Code code)
    {
        md.Code(code.code);
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Inheritance val)
    {
        md.Add("> ");
        if (val.inheritance.Length > 0)
            md.Add(val.inheritance[0]);
        for (int i = 1; i < val.inheritance.Length; i++)
            md.Add(" → ").Add(val.inheritance[i]);
        md.Para();
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, List val)
    {
        md.Add("> ");
        var vals = val.list;
        if (vals.Length > 0)
            md.Add(vals[0]);
        for (int i = 1; i < vals.Length; i++)
            md.Add(", ").Add(vals[i]);
        md.Para();
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Api2 val)
    {
        md.H2(val.api2);
        if (val.src != null)
            md.SrcLink(val.src);
        if (val.deprecated != null)
            md.Bold("DEPRECATED").Para();
        if (val.preview != null)
            md.Bold("EXPERIMENTAL").Para();
        md.Para();
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Api3 val)
    {
        md.H3(val.api3);
        if (val.src != null)
            md.SrcLink(val.src);
        if (val.deprecated != null)
            md.Bold("DEPRECATED").Para();
        if (val.preview != null)
            md.Bold("EXPERIMENTAL").Para();
        md.Para();
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Api4 val)
    {
        md.H4(val.api4);
        if (val.src != null)
            md.SrcLink(val.src);
        if (val.deprecated != null)
            md.Bold("DEPRECATED").Para();
        if (val.preview != null)
            md.Bold("EXPERIMENTAL").Para();
        md.Para();
        return md;
    }

    internal static MDStringBuilder SrcLink(this MDStringBuilder md, string src)
    {
        md.Italic("Source:").Add(' ').Link(src, src[(src.LastIndexOf('/') + 1)..src.LastIndexOf('#')]).Break();
        return md;
    }

    internal static MDStringBuilder Add(this MDStringBuilder md, Parameters vals)
    {
        var arr = vals.parameters;
        if (arr.Length == 0)
            return md;
        if (arr[0].name != null)
        {
            using var table = md.Table();
            table.HeaderCell("Type");
            table.HeaderCell("Name");
            table.HeaderCell("Description");
            foreach (var item in arr)
            {
                table.BeginCell().Add(item.type); table.EndCell();
                table.Cell(item.name ?? string.Empty);
                table.BeginCell();
                if (item.preview != null)
                    md.Italic("EXPERIMENTAL ");
                if (item.deprecated != null)
                    md.Italic("DEPRECATED ");
                //if (item.optional ?? false)
                //    md.Italic("Optional, ");
                md.AddMarkdown(item.description?.Without('\n', '\r'));
                table.EndCell();
            }
        }
        else
        {
            using var table = md.Table();
            table.HeaderCell("Type");
            table.HeaderCell("Description");
            foreach (var item in arr)
            {
                table.BeginCell().Add(item.type); table.EndCell();
                table.BeginCell();
                if (item.preview != null)
                    md.Italic("EXPERIMENTAL ");
                if (item.deprecated != null)
                    md.Italic("DEPRECATED ");
                //if (item.optional ?? false)
                //    md.Italic("Optional, ");
                md.AddMarkdown(item.description?.Without('\n', '\r'));
                table.EndCell();
            }
        }
        return md;
    }
}
