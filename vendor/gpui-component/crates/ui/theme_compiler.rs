//! Build-time translation of the bundled JSON into typed theme definitions.
use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, quote};
use serde_json::Value;
use std::{collections::BTreeMap, env, fs, path::PathBuf};
use syn::{
    Expr, Ident, Item, LitStr, Pat, Stmt, Token,
    parse::{Parse, ParseStream},
};

struct Rule {
    field: Ident,
    fallback: Option<Expr>,
}
impl Parse for Rule {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let field = input.parse()?;
        let fallback = if input.is_empty() {
            None
        } else {
            input.parse::<Token![,]>()?;
            let keyword: Ident = input.parse()?;
            assert_eq!(keyword, "fallback");
            input.parse::<Token![=]>()?;
            Some(input.parse()?)
        };
        Ok(Self { field, fallback })
    }
}

// Built-in backgrounds are solid. Preserve the shared schema's fallback order
// and arithmetic while resolving its optional configuration at build time.
fn colors_receiver(tokens: TokenStream) -> TokenStream {
    tokens
        .into_iter()
        .map(|token| match token {
            TokenTree::Ident(ident) if ident == "self" || ident == "tokens" => TokenTree::Ident(Ident::new("colors", ident.span())),
            TokenTree::Group(group) => {
                let mut result = proc_macro2::Group::new(group.delimiter(), colors_receiver(group.stream()));
                result.set_span(group.span());
                TokenTree::Group(result)
            }
            other => other,
        })
        .collect()
}

fn hsla(h: f32, s: f32, l: f32, a: f32) -> TokenStream {
    assert!([h, s, l, a].iter().all(|v| v.is_finite()), "non-finite bundled color");
    quote!(gpui::Hsla { h: #h, s: #s, l: #l, a: #a })
}
fn palette_color(value: &Value) -> TokenStream {
    let parts = value["hslChannel"].as_str().expect("palette hslChannel").split_whitespace().map(|s| s.trim_end_matches('%').parse::<f32>().expect("palette channel number")).collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    hsla(parts[0] / 360.0, parts[1] / 100.0, parts[2] / 100.0, 1.0)
}
fn color(value: &str, palette: &Value) -> TokenStream {
    if let Some(hex) = value.strip_prefix('#') {
        assert!(hex.len() == 6 || hex.len() == 8, "bundled hex color: {value}");
        let rgb = (0..3).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex channel") as f32 / 255.0).collect::<Vec<_>>();
        let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
        let max = r.max(g.max(b));
        let min = r.min(g.min(b));
        let delta = max - min;
        let l = (max + min) / 2.0;
        let s = if l == 0.0 || l == 1.0 {
            0.0
        } else if l < 0.5 {
            delta / (2.0 * l)
        } else {
            delta / (2.0 - 2.0 * l)
        };
        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0) / 6.0
        } else if max == g {
            ((b - r) / delta + 2.0) / 6.0
        } else {
            ((r - g) / delta + 4.0) / 6.0
        };
        let a = if hex.len() == 8 { u8::from_str_radix(&hex[6..8], 16).expect("hex alpha") as f32 / 255.0 } else { 1.0 };
        return hsla(h, s, l, a);
    }
    if value == "transparent" {
        return hsla(0.0, 0.0, 0.0, 0.0);
    }
    if let Some((name, scale)) = value.rsplit_once('-') {
        let scale = scale.parse::<u64>().expect("bundled color scale");
        let found = palette[name].as_array().expect("bundled palette name").iter().find(|row| row["scale"].as_u64() == Some(scale)).expect("bundled palette scale");
        return palette_color(found);
    }
    palette_color(&palette[value])
}

pub fn compile() {
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("src/theme");
    for name in ["default-theme.json", "default-colors.json", "schema.rs"] {
        println!("cargo:rerun-if-changed={}", directory.join(name).display());
    }
    println!("cargo:rerun-if-changed=theme_compiler.rs");
    let palette: Value = serde_json::from_str(&fs::read_to_string(directory.join("default-colors.json")).unwrap()).expect("valid bundled palette JSON");
    let themes: Value = serde_json::from_str(&fs::read_to_string(directory.join("default-theme.json")).unwrap()).expect("valid bundled theme JSON");
    let schema = syn::parse_file(&fs::read_to_string(directory.join("schema.rs")).unwrap()).expect("theme schema Rust");
    let fields = schema
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(item) if item.ident == "ThemeConfigColors" => Some(&item.fields),
            _ => None,
        })
        .unwrap();
    let mut names = BTreeMap::new();
    for field in fields {
        let ident = field.ident.as_ref().unwrap();
        let mut name = ident.to_string();
        for attribute in &field.attrs {
            if attribute.path().is_ident("serde") {
                attribute
                    .parse_nested_meta(|meta| {
                        if meta.path.is_ident("rename") {
                            name = meta.value()?.parse::<LitStr>()?.value();
                        }
                        Ok(())
                    })
                    .unwrap();
            }
        }
        names.insert(name, ident.clone());
    }
    let method = schema
        .items
        .iter()
        .find_map(|item| match item {
            Item::Impl(item) if item.self_ty.to_token_stream().to_string() == "ThemeColor" => item.items.iter().find_map(|item| match item {
                syn::ImplItem::Fn(method) if method.sig.ident == "apply_config" => Some(method),
                _ => None,
            }),
            _ => None,
        })
        .unwrap();
    let mut constructors = TokenStream::new();
    let mut configs = Vec::new();
    let mut settings = Vec::new();
    for theme in themes["themes"].as_array().expect("bundled themes") {
        let dark = match theme["mode"].as_str().unwrap() {
            "dark" => true,
            "light" => false,
            _ => panic!("bundled theme mode"),
        };
        let function = Ident::new(if dark { "dark" } else { "light" }, proc_macro2::Span::call_site());
        let mode = if dark { quote!(super::ThemeMode::Dark) } else { quote!(super::ThemeMode::Light) };
        let mut configured = BTreeMap::new();
        let mut config_colors = Vec::new();
        for (name, raw) in theme["colors"].as_object().expect("theme colors") {
            // Match Serde's handling of legacy keys left in bundled themes.
            let Some(field) = names.get(name) else { continue };
            let value = raw.as_str().expect("theme color string");
            configured.insert(field.to_string(), color(value, &palette));
            config_colors.push(quote!(#field: Some(#value.into())));
        }
        let mut assignments = TokenStream::new();
        for statement in &method.block.stmts {
            match statement {
                Stmt::Macro(statement) if matches!(statement.mac.path.to_token_stream().to_string().as_str(), "apply_color" | "apply_background_color") => {
                    let rule = syn::parse2::<Rule>(statement.mac.tokens.clone()).expect("theme fallback rule");
                    let field = rule.field;
                    let value = configured.get(&field.to_string()).cloned().unwrap_or_else(|| rule.fallback.map(|e| colors_receiver(e.into_token_stream())).unwrap_or_else(|| quote!(gpui::Hsla::default())));
                    assignments.extend(quote!(colors.#field = #value;));
                }
                Stmt::Local(local) => {
                    if let Pat::Ident(pattern) = &local.pat {
                        if ["hover_opacity", "transparent", "button_background"].iter().any(|name| pattern.ident == *name) {
                            assignments.extend(colors_receiver(local.to_token_stream()));
                        }
                    }
                }
                _ => {}
            }
        }
        constructors.extend(quote! {
            pub(super) fn #function() -> super::ThemeColor {
                use super::Colorize as _;
                let mut colors = super::ThemeColor::default();
                let active_darken = if #dark { 0.2 } else { 0.1 };
                #assignments
                colors.list_active.a = colors.list_active.a.min(0.2);
                colors.table_active.a = colors.table_active.a.min(0.2);
                colors.selection.a = colors.selection.a.min(0.3);
                colors
            }
        });
        // The only config-dependent local left in the shared fallback rules.
        let generated = constructors.to_string().replace("config . mode . is_dark ()", if dark { "true" } else { "false" });
        constructors = generated.parse().unwrap();
        let name = theme["name"].as_str().unwrap();
        let is_default = theme["is_default"].as_bool().unwrap_or(false);
        let mut metadata = Vec::new();
        let mut applied = Vec::new();
        for (key, field) in [("font.size", "font_size"), ("font.family", "font_family"), ("mono_font.size", "mono_font_size"), ("mono_font.family", "mono_font_family"), ("radius", "radius"), ("radius.lg", "radius_lg"), ("shadow", "shadow")]
        {
            if let Some(value) = theme.get(key) {
                let field = Ident::new(field, proc_macro2::Span::call_site());
                let value = if let Some(text) = value.as_str() { quote!(#text.into()) } else { value.to_string().parse::<TokenStream>().unwrap() };
                metadata.push(quote!(#field: Some(#value)));
                let applied_value = if key.contains("size") || key.starts_with("radius") { quote!(gpui::px(#value as f32)) } else { value };
                applied.push(quote!(_theme.#field = #applied_value;));
            }
        }
        configs.push(quote! {
            super::ThemeConfig { name: #name.into(), mode: #mode, is_default: #is_default, #(#metadata,)*
                colors: {
                    #[cfg(feature = "json-themes")]
                    { super::ThemeConfigColors { #(#config_colors,)* ..Default::default() } }
                    #[cfg(not(feature = "json-themes"))]
                    { super::ThemeConfigColors::default() }
                }, ..Default::default()
            }
        });
        settings.push(quote!(#mode => { #(#applied)* }));
    }
    constructors.extend(quote! {
        pub(super) fn configs() -> Vec<super::ThemeConfig> { vec![#(#configs),*] }
        pub(super) fn settings(_theme: &mut super::Theme, mode: super::ThemeMode) { match mode { #(#settings),* } }
    });
    let mut palette_fields = Vec::new();
    for (name, value) in palette.as_object().unwrap() {
        if !value.is_array() && name != "black" && name != "white" {
            continue;
        }
        let field = Ident::new(name, proc_macro2::Span::call_site());
        let value = if let Some(rows) = value.as_array() {
            let mut rows = rows.iter().collect::<Vec<_>>();
            rows.sort_by_key(|row| row["scale"].as_u64().unwrap());
            let rows = rows.into_iter().map(|row| {
                let scale = row["scale"].as_u64().unwrap() as usize;
                let hsla = palette_color(row);
                quote!(ShadcnColor { scale: #scale, hsla: #hsla })
            });
            quote!(ColorScales(&[#(#rows),*]))
        } else {
            let hsla = palette_color(value);
            quote!(ShadcnColor { scale: 0, hsla: #hsla })
        };
        palette_fields.push(quote!(#field: #value));
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("compiled_themes.rs"), constructors.to_string()).unwrap();
    fs::write(out.join("compiled_palette.rs"), quote!(ShadcnColors { #(#palette_fields),* }).to_string()).unwrap();
}
