using Docfx.Build.ApiPage;
using OneOf;
using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq.Expressions;
using System.Reflection;
using System.Text;

namespace StarlightDocNet;

internal static partial class Dict2ObjExtensions
{
    public static T As<T>(this object dict)
    {
        if (dict is T conv)
            return conv;
        return Dict2Obj<T>.Convert((IDictionary<object, object>)dict);
    }

    public static T As<T>(this IDictionary<object, object> dict) => Dict2Obj<T>.Convert(dict);
    public static T[] AsArray<T>(this object list) => [.. ((IList<object>)list).Select(x => Convert<T>(x))];
    public static T[] AsArray<T>(this IList<object> list) => [.. list.Select(x => Convert<T>(x))];

    private static bool StrEquals(object? a, object? b)
    {
        if (a is string aStr && b is string bStr)
            return aStr.Equals(bStr);
        return false;
    }

    private static IEnumerable<KeyValuePair<T1, T2>> ConvertKVP<T1, T2>(IEnumerable<KeyValuePair<object, object>> src)
    {
        foreach (var kvp in src)
            yield return new KeyValuePair<T1, T2>((T1)kvp.Key, (T2)kvp.Value);
    }

    private static T[] ConvertArray<T>(object src) => [.. ((IList<object>)src).Select(x => Convert<T>(x))];

    private static T Convert<T>(object src)
    {
        if (src is T conv)
            return conv;
        if (typeof(T).IsAssignableTo(typeof(IOneOf)))
            return Obj2OneOf<T>.Convert(src);
        if (typeof(T).IsArray)
            return List2Obj<T>.Convert(src);
        return Dict2Obj<T>.Convert((IDictionary<object, object>)src);
    }
}

internal static class List2Obj<T>
{
    public delegate T ConvertDelegate(object src);
    private readonly static ConvertDelegate convertFunc;

    static List2Obj()
    {
        var type = typeof(T);
        var elemt = type.GetElementType()!;
        var convMeth = typeof(Dict2ObjExtensions).GetMethod("ConvertArray", BindingFlags.NonPublic | BindingFlags.Static);
        convMeth = convMeth!.MakeGenericMethod(elemt);

        var src = Expression.Parameter(typeof(object), "src");
        var result = Expression.Variable(typeof(T), "result");
        Expression body = Expression.Call(convMeth, src);

        convertFunc = Expression.Lambda<ConvertDelegate>(body, src).Compile();
    }

    public static T Convert(object src) => convertFunc(src);
}

internal static class Obj2OneOf<T>
// where T : IOneOf
{
    public delegate T ConvertDelegate(object src);
    private readonly static ConvertDelegate convertFunc;

    static Obj2OneOf()
    {
        var type = typeof(T);
        var types = type.BaseType!.GetGenericArguments();
        var oneoft = typeof(IOneOf).Assembly.GetType($"OneOf.OneOf`{types.Length}")!;
        oneoft = oneoft.MakeGenericType(types);
        var ctor = type.GetConstructor([oneoft])!;

        var src = Expression.Parameter(typeof(object), "src");
        var result = Expression.Variable(typeof(T), "result");
        Expression body = Expression.Empty();

        if (type.GetCustomAttribute<DiscriminateAttribute>() is DiscriminateAttribute discr)
        {
            var split = discr.Discriminator.Split('.');
            var dt = type.Assembly.GetType(type.Namespace + "." + split[0])!;
            var dm = dt.GetMethod(split[1], BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)!;
            var selT = Expression.Call(dm, src);
            List<SwitchCase> tSwitchCases = [];
            for (int i = 0; i < types.Length; i++)
            {
                var from = oneoft.GetMethod($"FromT{i}", BindingFlags.Public | BindingFlags.Static)!;
                var convMeth = typeof(Dict2ObjExtensions).GetMethod("Convert", BindingFlags.Static | BindingFlags.NonPublic);
                convMeth = convMeth!.MakeGenericMethod(types[i]);
                tSwitchCases.Add(Expression.SwitchCase(Expression.Block(
                    Expression.Assign(result,
                        Expression.New(ctor, Expression.Call(from, Expression.Call(convMeth, src))))
                    , Expression.Empty()), Expression.Constant(i)));
            }
            body = Expression.Switch(selT, null, null, tSwitchCases);
        }
        else
        {
            for (int i = 0; i < types.Length; i++)
            {
                var from = oneoft.GetMethod($"FromT{i}", BindingFlags.Public | BindingFlags.Static)!;
                body = Expression.IfThenElse(Expression.Call(Expression.Constant(types[i]), "IsAssignableFrom", null, Expression.Call(src, "GetType", null)),
                    Expression.Assign(result, Expression.New(ctor,
                        Expression.Call(from, Expression.Convert(src, types[i])))),
                    body);
            }
        }

        var block = Expression.Block(typeof(T), [result], result, body, result);
        convertFunc = Expression.Lambda<ConvertDelegate>(block, src).Compile();
    }

    public static T Convert(object src) => convertFunc(src);
}

internal static class Dict2Obj<T>
// where T : new()
{
    public delegate T ConvertDelegate(IDictionary<object, object> src);
    private readonly static ConvertDelegate convertFunc;

    static Dict2Obj()
    {
        // Linq expression magic to generate code to convert a string-keyed
        // dictionary into a proper object. Using expressions means we only
        // bear the cost of reflection once at startup and then very efficient
        // IL is generated. This is all kind of vibe-coded, and when I say
        // vibe-coded, I mean I was kind of just vibing as I typed this and
        // didn't give it too much thought (no ai here lol); it's a bit of a 
        // complicated mess lol.
        var props = typeof(T).GetProperties();
        var dictType = typeof(IDictionary<object, object>);

        if (typeof(T).IsAssignableTo(dictType))
        {
            convertFunc = x => (T)x;
            return;
        }

        var src = Expression.Parameter(dictType, "src");
        var result = Expression.Variable(typeof(T), "result");
        var it = Expression.Variable(typeof(IEnumerator<KeyValuePair<object, object>>), "it");
        var item = Expression.Variable(typeof(KeyValuePair<object, object>), "item");
        var val = Expression.Variable(typeof(object), "val");
        var valType = Expression.Variable(typeof(Type), "valType");

        if (typeof(T).IsAssignableTo(typeof(IDictionary)))
        {
            //Dictionary<object, object> o = new();
            //Dictionary<string, string> d = new(o.Select(x => new KeyValuePair<string, string>((string)x.Key, (string)x.Value)));
            var kvt = typeof(T).GenericTypeArguments;
            var kvpm = typeof(KeyValuePair).GetMethod("Create", BindingFlags.Static | BindingFlags.Public)!.MakeGenericMethod(kvt);
            var kvpt = kvpm.ReturnType;
            var kvpCtor = kvpt.GetConstructor(kvt)!;
            var enumt = typeof(IEnumerable<>).MakeGenericType(kvpt);
            var ctor = typeof(T).GetConstructor([enumt])!;
            var conv = typeof(Dict2ObjExtensions).GetMethod("ConvertKVP", BindingFlags.Static | BindingFlags.NonPublic)!;
            conv = conv.MakeGenericMethod(kvt);
            var convParam = Expression.Parameter(dictType, "src");
            var convFunc = Expression.New(ctor, Expression.Call(conv, convParam));

            convertFunc = Expression.Lambda<ConvertDelegate>(convFunc, convParam).Compile();
            return;
        }

        var getEnumMeth = typeof(IEnumerable<KeyValuePair<object, object>>).GetMethod("GetEnumerator")!;
        var moveNextMeth = typeof(IEnumerator).GetMethod("MoveNext")!;
        var strEqMeth = typeof(Dict2ObjExtensions).GetMethod("StrEquals", BindingFlags.Static | BindingFlags.NonPublic);
        var convArrayMeth = typeof(Dict2ObjExtensions).GetMethod("AsArray", BindingFlags.Static | BindingFlags.Public, [typeof(object)])!;

        var breakLab = Expression.Label();
        List<Expression> statements = [];

        statements.Add(Expression.Assign(result, Expression.New(typeof(T))));
        statements.Add(Expression.Assign(it, Expression.Call(src, getEnumMeth)));

        List<Expression> loopCont = [];
        loopCont.Add(Expression.IfThen(Expression.Not(Expression.Call(it, moveNextMeth)),
            Expression.Break(breakLab)));
        loopCont.Add(Expression.Assign(item, Expression.Property(it, "Current")));
        loopCont.Add(Expression.Assign(val, Expression.Property(item, "Value")));
        loopCont.Add(Expression.Assign(valType, Expression.Call(val, "GetType", null)));

        List<SwitchCase> cases = [];
        foreach (var prop in props)
        {
            if (!prop.CanRead || !prop.CanWrite)
                continue;

            var propType = prop.PropertyType;
            var propTypeExpr = Expression.Constant(propType);

            Expression switchContents;
            if (propType.IsAssignableTo(typeof(IOneOf)))
            {
                // result.prop = Obj2OneOf<T>.Convert(val);
                var obj2oneoft = typeof(Obj2OneOf<>).MakeGenericType(propType);
                var convertMeth = obj2oneoft.GetMethod("Convert", BindingFlags.Static | BindingFlags.Public)!;
                switchContents = Expression.Block(Expression.Assign(Expression.Property(result, prop), Expression.Call(convertMeth, val)), Expression.Empty());
            }
            else if (propType.IsArray)
            {
                // result.prop = ExtensionMethods.ToArray<T>(val);
                var elemType = propType.GetElementType()!;
                var toArray = convArrayMeth.MakeGenericMethod(elemType);
                switchContents = Expression.Block(Expression.Assign(Expression.Property(result, prop), Expression.Call(toArray, val)), Expression.Empty());
            }
            else
            {
                var d2oProp = typeof(Dict2Obj<>).MakeGenericType(propType).GetMethod("Convert", BindingFlags.Static | BindingFlags.Public);
                switchContents =
                    Expression.IfThenElse(Expression.Call(valType, "IsAssignableTo", null, propTypeExpr),
                        Expression.Assign(Expression.Property(result, prop), Expression.Convert(val, propType)),
                        Expression.IfThen(Expression.Call(valType, "IsAssignableTo", null, Expression.Constant(dictType)),
                            Expression.Assign(Expression.Property(result, prop), Expression.Call(null, d2oProp!, Expression.Convert(val, dictType)))
            ));
            }
            cases.Add(Expression.SwitchCase(switchContents, Expression.Constant(prop.Name)));
        }

        loopCont.Add(Expression.Switch(Expression.Property(item, "Key"), null, strEqMeth, cases));
        statements.Add(Expression.Loop(Expression.Block(loopCont), breakLab));
        statements.Add(result);

        var block = Expression.Block(typeof(T), [result, it, item, val, valType], statements);
#if DEBUG && false
        var dbgView = typeof(Expression).GetProperty("DebugView", BindingFlags.Instance | BindingFlags.NonPublic)!;
        Debug.WriteLine($"### Generated Dict2Obj<{typeof(T).Name}> : \n{dbgView.GetValue(block)}\n");
#endif
        convertFunc = Expression.Lambda<ConvertDelegate>(block, src).Compile();
    }

    public static T Convert(IDictionary<object, object> src) => convertFunc(src);
}

[AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct)]
public sealed class DiscriminateAttribute(string discriminator) : Attribute
{
    public string Discriminator => discriminator;
}