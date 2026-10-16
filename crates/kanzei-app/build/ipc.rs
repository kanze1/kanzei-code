use heck::ToLowerCamelCase;
use quote::{format_ident, quote, ToTokens};
use std::{collections::BTreeMap, fs, path::Path};
use syn::{visit_mut::VisitMut, FnArg, Item, Type, UseTree};

fn uses(tree: &UseTree, prefix: String, out: &mut BTreeMap<String, String>) {
    match tree {
        UseTree::Path(p) => uses(&p.tree, format!("{prefix}{}::", p.ident), out),
        UseTree::Name(n) => {
            out.insert(n.ident.to_string(), format!("{prefix}{}", n.ident));
        }
        UseTree::Rename(n) => {
            out.insert(n.rename.to_string(), format!("{prefix}{}", n.ident));
        }
        UseTree::Group(g) => {
            for tree in &g.items {
                uses(tree, prefix.clone(), out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

struct Qualify<'a> {
    module: &'a str,
    imports: &'a BTreeMap<String, String>,
}
impl VisitMut for Qualify<'_> {
    fn visit_type_path_mut(&mut self, node: &mut syn::TypePath) {
        syn::visit_mut::visit_type_path_mut(self, node);
        if node.qself.is_some() {
            return;
        }
        let first = node
            .path
            .segments
            .first()
            .expect("type path")
            .ident
            .to_string();
        let builtin = [
            "String", "Vec", "Option", "Result", "bool", "u8", "u16", "u32", "u64", "i32", "i64",
            "usize", "f32", "f64", "str",
        ];
        let replacement = if let Some(import) = self.imports.get(&first) {
            import.clone()
        } else if builtin.contains(&first.as_str())
            || ["crate", "std", "serde_json", "tauri"].contains(&first.as_str())
            || first.starts_with("kanzei_")
        {
            return;
        } else {
            format!(
                "crate::{}{}",
                if self.module.is_empty() {
                    String::new()
                } else {
                    format!("{}::", self.module)
                },
                first
            )
        };
        let mut replacement = replacement;
        if replacement.starts_with("super::") || replacement.starts_with("self::") {
            let mut parts: Vec<_> = self.module.split("::").filter(|p| !p.is_empty()).collect();
            while let Some(rest) = replacement.strip_prefix("super::") {
                parts.pop();
                replacement = rest.to_owned();
            }
            replacement = replacement
                .strip_prefix("self::")
                .unwrap_or(&replacement)
                .to_owned();
            replacement = format!(
                "crate::{}{}",
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("{}::", parts.join("::"))
                },
                replacement
            );
        }
        let mut path: syn::Path = syn::parse_str(&replacement).expect("qualified import");
        path.segments.last_mut().unwrap().arguments =
            node.path.segments.first().unwrap().arguments.clone();
        path.segments
            .extend(node.path.segments.iter().skip(1).cloned());
        node.path = path;
    }
}

pub fn generate() {
    println!("cargo:rerun-if-changed=build/ipc.rs");
    println!("cargo:rerun-if-changed=src/command_registry.json");
    let registry: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string("src/command_registry.json").unwrap()).unwrap();
    let mut arms = Vec::new();
    let mut paths = Vec::new();
    let mut desktop_names = Vec::new();
    let mut manifest = BTreeMap::new();
    for entry in registry {
        let path = entry["path"].as_str().expect("command path");
        let (module, name) = path.rsplit_once("::").unwrap_or(("", path));
        let base = if module.is_empty() {
            "src/main".to_owned()
        } else {
            format!("src/{}", module.replace("::", "/"))
        };
        let filename = if Path::new(&format!("{base}.rs")).is_file() {
            format!("{base}.rs")
        } else {
            format!("{base}/mod.rs")
        };
        let source = syn::parse_file(
            &fs::read_to_string(&filename).unwrap_or_else(|e| panic!("{path}: {e}")),
        )
        .unwrap();
        let mut imports = BTreeMap::new();
        for item in &source.items {
            if let Item::Use(item) = item {
                uses(&item.tree, String::new(), &mut imports);
            }
        }
        let function = source
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(f) if f.sig.ident == name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("unresolved registered command {path}"));
        let mut command = name.to_owned();
        let mut snake = false;
        let attribute = function
            .attrs
            .iter()
            .find(|a| a.path().to_token_stream().to_string() == "tauri :: command")
            .expect("registered function must be a tauri command");
        if let syn::Meta::List(list) = &attribute.meta {
            use syn::ext::IdentExt;
            list.parse_args_with(|input: syn::parse::ParseStream| {
                while !input.is_empty() {
                    let key = input.call(syn::Ident::parse_any)?;
                    if input.peek(syn::Token![=]) {
                        input.parse::<syn::Token![=]>()?;
                        let value: syn::LitStr = input.parse()?;
                        if key == "rename" {
                            command = value.value();
                        } else if key == "rename_all" {
                            let case = value.value();
                            assert!(case == "snake_case" || case == "camelCase");
                            snake = case == "snake_case";
                        } else {
                            panic!("unsupported command attribute {key}");
                        }
                    } else {
                        assert!(key == "async", "unsupported command attribute {key}");
                    }
                    if !input.is_empty() {
                        input.parse::<syn::Token![,]>()?;
                    }
                }
                Ok(())
            })
            .expect("command attributes");
        }
        let command_path: syn::Path = syn::parse_str(&format!("crate::{path}")).unwrap();
        paths.push(command_path.clone());
        let desktop = entry["host"] == "desktop";
        assert!(
            desktop || entry["host"] == "service",
            "invalid host for {path}"
        );
        let mut fields = Vec::new();
        let mut values = Vec::new();
        let mut params = BTreeMap::new();
        for arg in &function.sig.inputs {
            let FnArg::Typed(arg) = arg else {
                panic!("command with receiver")
            };
            let syn::Pat::Ident(pattern) = &*arg.pat else {
                panic!("command pattern")
            };
            let ident = &pattern.ident;
            let type_name = match &*arg.ty {
                Type::Path(p) => p.path.segments.last().unwrap().ident.to_string(),
                _ => String::new(),
            };
            if type_name == "State" {
                values.push(quote! { app.state() });
                continue;
            }
            if ["AppHandle", "AppEvents"].contains(&type_name.as_str()) {
                values.push(quote! { app.clone() });
                continue;
            }
            if ["Window", "WebviewWindow", "Webview"].contains(&type_name.as_str()) {
                assert!(desktop, "service command {path} still requires a window");
                continue;
            }
            let field_name = ident.to_string();
            let wire_name = if snake {
                field_name.trim_start_matches('_').to_owned()
            } else {
                field_name.trim_start_matches('_').to_lower_camel_case()
            };
            let mut ty = (*arg.ty).clone();
            Qualify {
                module,
                imports: &imports,
            }
            .visit_type_mut(&mut ty);
            params.insert(wire_name.clone(), serde_json::json!({"type": ty.to_token_stream().to_string(), "required": type_name != "Option"}));
            fields.push(quote! { #[serde(rename = #wire_name)] #ident: #ty });
            values.push(quote! { args.#ident });
        }
        let output = function.sig.output.to_token_stream().to_string();
        let data = serde_json::json!({"path":path,"host":entry["host"],"parameters":params,"returns":output});
        assert!(
            manifest.insert(command.clone(), data).is_none(),
            "duplicate command {command}"
        );
        if desktop {
            desktop_names.push(command);
            continue;
        }
        let argument_type = format_ident!("Arguments");
        let call = quote! { #command_path(#(#values),*) };
        let call = if function.sig.asyncness.is_some() {
            quote! { #call.await }
        } else {
            call
        };
        let returns_result = match &function.sig.output {
            syn::ReturnType::Type(_, ty) => {
                matches!(&**ty, Type::Path(p) if p.path.segments.last().unwrap().ident == "Result")
            }
            _ => false,
        };
        let result_type = match &function.sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, ty) if returns_result => {
                let Type::Path(path) = &**ty else {
                    unreachable!()
                };
                let syn::PathArguments::AngleBracketed(arguments) =
                    &path.path.segments.last().unwrap().arguments
                else {
                    unreachable!()
                };
                arguments.args.iter().find_map(|arg| {
                    if let syn::GenericArgument::Type(ty) = arg {
                        Some(ty)
                    } else {
                        None
                    }
                })
            }
            syn::ReturnType::Type(_, ty) => Some(&**ty),
        };
        let unit =
            result_type.is_none_or(|ty| matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty()));
        let result = if returns_result {
            quote! { #call.map_err(|error| error.to_string())? }
        } else {
            call
        };
        let body = if unit {
            quote! { #result; Ok(serde_json::Value::Null) }
        } else {
            quote! { serde_json::to_value(#result).map_err(|error| error.to_string()) }
        };
        let body = if function.sig.asyncness.is_some() {
            body
        } else {
            quote! { tauri::async_runtime::spawn_blocking(move || { #body }).await.map_err(|error| format!("command worker failed: {error}"))? }
        };
        arms.push(quote! { #command => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct #argument_type { #(#fields),* }
            let args: #argument_type = serde_json::from_value(args).map_err(|error| format!("invalid {} arguments: {error}", #command))?;
            let _ = &args;
            #body
        } });
    }
    let generated = quote! {
        pub(crate) fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static { tauri::generate_handler![#(#paths),*] }
        pub(crate) fn desktop_command(command: &str) -> bool { command.starts_with("plugin:") || matches!(command, #(#desktop_names)|*) }
        pub(crate) async fn dispatch(command: &str, args: serde_json::Value, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
            use tauri::Manager;
            match command { #(#arms),*, _ => Err(format!("command is not available in the runtime service: {command}")) }
        }
        #[cfg(test)]
        mod generated_contract_tests {
            #[test]
            fn checked_in_parameter_contract_matches_rust_signatures() {
                let generated: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/ipc-commands.json"))).unwrap();
                let checked: serde_json::Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/ipc-commands.json"))).unwrap();
                assert_eq!(generated, checked, "Run node scripts/sync-ipc-contract.mjs after building to update the generated command contract");
            }
        }
    };
    let out = std::env::var("OUT_DIR").unwrap();
    fs::write(
        Path::new(&out).join("native_dispatch.rs"),
        generated.to_string(),
    )
    .unwrap();
    fs::write(
        Path::new(&out).join("ipc-commands.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}
