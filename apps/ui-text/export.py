#!/usr/bin/env python3
"""Export JavaScript and Android copy from the app catalog."""

import argparse
import html
import json
from pathlib import Path
import re
from tempfile import TemporaryDirectory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, help="Web bundle directory (defaults to web-gpui source)")
    parser.add_argument("--check", action="store_true", help="Check committed browser outputs without writing")
    parser.add_argument("--text-output", type=Path, help="Also export all catalog values as plain text")
    platforms = parser.add_mutually_exclusive_group()
    platforms.add_argument("--web-only", action="store_true")
    platforms.add_argument("--android-only", action="store_true")
    args = parser.parse_args()
    catalog_dir = Path(__file__).resolve().parent
    web_dir = catalog_dir.parent / "web-gpui"
    output_dir = args.output_dir or web_dir
    catalog = json.loads((catalog_dir / "english.json").read_text())
    values = {f"{section}.{key}": value for section, entries in catalog.items() for key, value in entries.items()}
    outputs = {}
    if not args.android_only:
        outputs.update(web_outputs(web_dir, output_dir, values))
    if not args.web_only and not args.output_dir:
        outputs.update(android_outputs(catalog_dir.parent / "desktop-gpui/android", values))
    if args.text_output:
        outputs[args.text_output] = "\n".join(dict.fromkeys(value.replace("\n", "\\n") for value in values.values())) + "\n"
    for path, content in outputs.items():
        if args.check:
            if not path.exists() or path.read_text() != content:
                raise SystemExit(f"UI text export is stale: {path}; run apps/ui-text/export.py")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists() or path.read_text() != content:
                # Keep the previous output intact if storage fills during export.
                with TemporaryDirectory(prefix=".ui-text-", dir=path.parent) as directory:
                    temporary = Path(directory) / path.name
                    temporary.write_text(content)
                    temporary.chmod(path.stat().st_mode & 0o777 if path.exists() else 0o644)
                    temporary.replace(path)


def javascript_messages(entries, function_name, dictionary_name):
    module = "const " + dictionary_name + " = " + json.dumps(entries, ensure_ascii=False, indent=4) + ";\n\n"
    module += """function FUNCTION(key, parameters = {}) {
    const message = DICTIONARY[key];
    if (message === undefined) throw new Error(`Unknown UI text key: ${key}`);
    return message.replace(/\\{([a-zA-Z_][a-zA-Z0-9_]*)\\}/g, (_, name) => {
        if (!(name in parameters)) throw new Error(`Missing UI text parameter: ${key}.${name}`);
        return String(parameters[name]);
    });
}
""".replace("FUNCTION", function_name).replace("DICTIONARY", dictionary_name)
    return module


def web_outputs(web_dir, output_dir, values):
    # Only include messages used by the small browser startup modules.
    # Older HTML can finish loading its startup module during deployment.
    used = {"web.opening_bokheim", "web.retrying", "web.downloading_app", "web.downloading_app_progress"}
    for name in ("startup.mjs", "compatibility.mjs", "tabs.mjs"):
        used.update(re.findall(r"\btext\('([a-z0-9_.]+)'", (web_dir / name).read_text()))
    entries = {key: values[key] for key in sorted(used)}
    module = "// Generated from apps/ui-text/english.json by export.py.\n"
    module += javascript_messages(entries, "text", "messages").replace("function text(", "export function text(", 1)
    source = (web_dir / "index.html").read_text()
    source = re.sub(
        r'(<[a-z]+\b[^>]*\bdata-ui-text="([a-z0-9_.]+)"[^>]*>)[^<]*(</[a-z]+>)',
        lambda match: match[1] + html.escape(values[match[2]]) + match[3],
        source,
    )
    outputs = {output_dir / "ui-text.mjs": module, output_dir / "index.html": source}
    authentication = web_dir.parent / "browser/web/authentication.js"
    source = authentication.read_text()
    used = sorted(set(re.findall(r"\buiText\('([a-z0-9_.]+)'", source)))
    block = "// BEGIN GENERATED UI TEXT\n// From apps/ui-text/english.json; run export.py to update.\n"
    block += javascript_messages({key: values[key] for key in used}, "uiText", "authenticationMessages")
    block += "// END GENERATED UI TEXT"
    outputs[authentication] = re.sub(r"// BEGIN GENERATED UI TEXT.*?// END GENERATED UI TEXT", lambda _: block, source, count=1, flags=re.S)
    return outputs


def android_outputs(android_dir, values):
    source_dir = android_dir / "src/main/java/se/bokheim/reader/gpui"
    constants = {key.replace(".", "_").upper(): value for key, value in values.items()}
    assert len(constants) == len(values), "UI text keys collide as Java constants"
    used = set()
    for path in source_dir.glob("*.java"):
        if path.name != "UiText.java":
            used.update(re.findall(r"\bUiText\.([A-Z][A-Z0-9_]+)", path.read_text()))
    source = "// Generated from apps/ui-text/english.json by export.py.\n"
    source += "package se.bokheim.reader.gpui;\n\nfinal class UiText {\n"
    source += "    private UiText() {}\n\n"
    for key in sorted(used):
        source += f"    static final String {key} = {json.dumps(constants[key], ensure_ascii=False)};\n"
    source += """
    static String format(String template, Object... parameters) {
        if (parameters.length % 2 != 0) throw new IllegalArgumentException("Expected name/value pairs");
        java.util.regex.Matcher matcher = java.util.regex.Pattern.compile("\\\\{([a-zA-Z_][a-zA-Z0-9_]*)\\\\}").matcher(template);
        StringBuffer result = new StringBuffer();
        while (matcher.find()) {
            String name = matcher.group(1);
            Object value = null;
            boolean found = false;
            for (int index = 0; index < parameters.length; index += 2) {
                if (name.equals(parameters[index])) {
                    value = parameters[index + 1];
                    found = true;
                    break;
                }
            }
            if (!found) throw new IllegalArgumentException("Missing UI text parameter: " + name);
            matcher.appendReplacement(result, java.util.regex.Matcher.quoteReplacement(String.valueOf(value)));
        }
        matcher.appendTail(result);
        return result.toString();
    }
}
"""
    return {source_dir / "UiText.java": source}


if __name__ == "__main__":
    main()
