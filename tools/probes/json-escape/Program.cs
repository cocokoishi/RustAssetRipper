// What exactly does `HttpUtility.JavaScriptStringEncode` escape?
//
// `DefaultJsonWalker` uses two overloads and their behaviour decides whether the Rust port can
// reuse its existing `escape_json` helper or needs a separate one:
//   * `JavaScriptStringEncode(name, true)` in `EnterField` - the overload that adds the quotes,
//     so field names are fully encoded;
//   * `JavaScriptStringEncode(value)` in `VisitPrimitive` - the overload that does NOT add quotes,
//     because the walker writes them itself.
//
// Run (from the RustAssetRipper repository root):
//   dotnet run --project tools/probes/json-escape/JsonEscapeProbe.csproj -c Release
using System.Web;

public static class Program
{
    public static int Main()
    {
        // Characters chosen because they separate the plausible escaping rules: JSON-mandatory,
        // HTML-sensitive, Unicode line terminators, and plain text.
        string[] samples =
        {
            "plain",
            "a\"b",
            "a\\b",
            "a/b",
            "a\tb",
            "a\nb",
            "a\rb",
            "a\u0008b",
            "a\u000cb",
            "a\u0001b",
            "a\u001fb",
            "a<b>c",
            "a&b",
            "a'b",
            "a\u2028b",
            "a\u2029b",
            "\u00e9\u4e2d",
        };

        foreach (string sample in samples)
        {
            Console.WriteLine($"NOQUOTE|{Escape(sample)}|{Describe(sample)}");
        }
        Console.WriteLine("---");
        foreach (string sample in samples)
        {
            Console.WriteLine($"QUOTED|{EscapeWithQuotes(sample)}|{Describe(sample)}");
        }
        return 0;
    }

    private static string Escape(string value) => HttpUtility.JavaScriptStringEncode(value);

    private static string EscapeWithQuotes(string value) =>
        HttpUtility.JavaScriptStringEncode(value, true);

    /// A stable, printable name for the sample so the comparison rows are readable.
    private static string Describe(string value)
    {
        var builder = new System.Text.StringBuilder();
        foreach (char c in value)
        {
            if (c < 0x20 || c > 0x7e)
            {
                builder.Append($"\\u{(int)c:X4}");
            }
            else
            {
                builder.Append(c);
            }
        }
        return builder.ToString();
    }
}
