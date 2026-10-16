// Dumps the frozen build's baked LanguageNameDictionary for comparison with the
// port's vendored language_names.json.
//
// Usage:
//   dotnet run --project tools/probes/lang-names/LangNamesProbe.csproj -c Release
using System.Reflection;

Assembly assembly = typeof(AssetRipper.GUI.Localizations.Localization).Assembly;
Type codes = assembly.GetType("AssetRipper.GUI.Localizations.LanguageCodes")!;
object? dictionary = codes.GetProperty("LanguageNameDictionary")?.GetValue(null)
	?? codes.GetField("LanguageNameDictionary")?.GetValue(null);
if (dictionary is not System.Collections.IDictionary entries)
{
	Console.Error.WriteLine("LanguageNameDictionary not found");
	return 2;
}

foreach (System.Collections.DictionaryEntry entry in entries)
{
	Console.WriteLine($"LANG|{entry.Key}|{entry.Value}");
}
Console.WriteLine($"COUNT|{entries.Count}");
return 0;
