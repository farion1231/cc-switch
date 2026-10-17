//! `#[command_api(rest = "METHOD /path", path = "id", body = "a,b", query = "c", state = "service,app_state")]`
//!
//! **单一真相源 → 双生成**:一个纯 fn,宏生成三件套:
//! 1. `<name>_impl` —— 原函数体改名(取 `&AppState`/`&SkillServiceState` 等,纯函数,可单测)
//! 2. `<name>` —— `#[tauri::command]` 适配(state.inner() + AppError→String)
//! 3. (仅 `rest_api` feature)`<name>_rest` —— axum handler,按 path/query/body 取参
//!
//! 约定:`state` 属性列出 state 参数名(缺省时仅 `state` 参被视为 state);
//! state 参数类型从 `&T` 注解提取,router 状态为 `RestState`,各子状态经
//! `FromRef<RestState>` 暴露给 axum `State<T>` 提取器。
//! 其余参数按 `path`/`body`/`query` 属性路由;路径中 `{id}` 自动转 axum `:id`。

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    FnArg, ItemFn, LitStr, Pat, Result, Token, Type,
};

/// 属性:`rest = "POST /p", path = "id", body = "a,b", query = "c", state = "service,app_state"`
struct CommandApiAttr {
    rest: Option<String>,
    path: Option<String>,
    body: Option<String>,
    query: Option<String>,
    state: Option<String>,
}

impl Parse for CommandApiAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut rest = None;
        let mut path = None;
        let mut body = None;
        let mut query = None;
        let mut state = None;
        while !input.is_empty() {
            let k: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let v: LitStr = input.parse()?;
            match k.to_string().as_str() {
                "rest" => rest = Some(v.value()),
                "path" => path = Some(v.value()),
                "body" => body = Some(v.value()),
                "query" => query = Some(v.value()),
                "state" => state = Some(v.value()),
                other => {
                    return Err(syn::Error::new(k.span(), format!("未知属性 {other}")))
                }
            }
            let _ = input.parse::<Token![,]>();
        }
        Ok(Self { rest, path, body, query, state })
    }
}

/// `(name, optional rename)`；`"app_type as app"` → (`app_type`, Some(`app`))
fn split_names(s: &Option<String>) -> Vec<(String, Option<String>)> {
    s.as_deref()
        .unwrap_or("")
        .split(',')
        .map(|x| {
            let x = x.trim();
            if x.is_empty() {
                return (String::new(), None);
            }
            match x.split_once(" as ") {
                Some((n, r)) => (n.trim().to_string(), Some(r.trim().to_string())),
                None => (x.to_string(), None),
            }
        })
        .filter(|(n, _)| !n.is_empty())
        .collect()
}

/// 调用参槽位:state 参数(按 ident 填充提取器变量)或普通参数(按 path/body/query 取字段)。
enum Slot {
    State(syn::Ident),
    Arg(syn::Ident),
}

#[proc_macro_attribute]
pub fn command_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = parse_macro_input!(attr as CommandApiAttr);
    let mut func = parse_macro_input!(item as ItemFn);

    let name = func.sig.ident.clone();
    let impl_name = format_ident!("{}_impl", name);
    let is_async = func.sig.asyncness.is_some();

    // state 参数名集合:显式 `state="service,app_state"` 或缺省退化为 ["state"]。
    let state_idents: Vec<String> = match &attr.state {
        Some(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        None => vec!["state".to_string()],
    };

    // 分离 state 参数与其余参数(保留声明顺序)
    let mut state_params: Vec<(syn::Ident, Type)> = Vec::new();
    let mut other_args: Vec<(syn::Ident, Type)> = Vec::new();
    for arg in &func.sig.inputs {
        if let FnArg::Typed(pt) = arg {
            let ty = &pt.ty;
            if let Pat::Ident(pi) = &*pt.pat {
                if state_idents.iter().any(|s| s == &pi.ident.to_string()) {
                    state_params.push((pi.ident.clone(), strip_ref(ty)));
                } else {
                    other_args.push((pi.ident.clone(), (**ty).clone()));
                }
            }
        }
    }
    let has_state = !state_params.is_empty();
    // router 状态类型始终为 RestState(复合:AppState + 自定义 state Arc)。
    let router_ty: Type = syn::parse_quote!(crate::rest::service::RestState);

    let (is_result_ret, ret_ty, ok_ty) = analyze_return(&func.sig.output);

    // —— 1. impl(改名,原函数体)——
    func.sig.ident = impl_name.clone();
    let attrs = &func.attrs;
    let vis = &func.vis;
    let sig = &func.sig;
    let block = &func.block;
    let impl_fn = quote! {
        #(#attrs)*
        #vis #sig #block
    };

    // —— 2. Tauri command 适配(Tauri 端不需路由,所有参从 invoke args 取)——
    // 按原始签名顺序的调用参槽位
    let call_slots: Vec<Slot> = func.sig.inputs.iter().filter_map(|arg| {
        if let FnArg::Typed(pt) = arg {
            if let Pat::Ident(pi) = &*pt.pat {
                if state_idents.iter().any(|s| s == &pi.ident.to_string()) {
                    return Some(Slot::State(pi.ident.clone()));
                }
                return Some(Slot::Arg(pi.ident.clone()));
            }
        }
        None
    }).collect();

    let other_inputs: TokenStream2 = other_args
        .iter()
        .map(|(i, t)| quote! { #i: #t, })
        .collect();
    // 每个 state 参数生成一个 tauri::State<'_, #ty> 声明
    let tauri_state_params: TokenStream2 = state_params
        .iter()
        .map(|(i, t)| quote! { #i: tauri::State<'_, #t>, })
        .collect();
    let await_tok = if is_async { Some(quote! {.await}) } else { None };
    let async_kw = if is_async { Some(quote! {async}) } else { None };
    // Tauri 调用参:state 槽 → #ident.inner();普通槽 → #ident
    let tauri_call: TokenStream2 = call_slots.iter().map(|slot| match slot {
        Slot::State(i) => { let t = i.clone(); quote! { #t.inner(), } }
        Slot::Arg(i) => { let t = i.clone(); quote! { #t, } }
    }).collect();
    let tauri_body = if is_result_ret {
        quote! {
            #impl_name(#tauri_call) #await_tok
                .map_err(|e| e.to_string())
        }
    } else {
        quote! { #impl_name(#tauri_call) #await_tok }
    };
    let tauri_ret_ty = if is_result_ret {
        quote! { std::result::Result<#ok_ty, String> }
    } else {
        quote! { #ret_ty }
    };
    let tauri_cmd = quote! {
        #[tauri::command]
        #vis #async_kw fn #name(
            #tauri_state_params
            #other_inputs
        ) -> #tauri_ret_ty {
            #tauri_body
        }
    };

    // —— 3. axum handler(仅 rest_api feature)——
    // rest 可选：缺省时 method=POST、path=/control/v1/<fn_name>（零配置即暴露）
    let (method, axum_path) = match &attr.rest {
        Some(rest_str) => parse_rest_route(rest_str),
        None => ("POST".to_string(), format!("/control/v1/{}", name)),
    };
    let rest_handler = {
    let upper = name.to_string().to_uppercase();
        let path_names = split_names(&attr.path);
        let body_names = split_names(&attr.body);
        // 零配置（path/body/query 均缺省）：全部非-state 参 → body（camelCase）
        let all_default = attr.path.is_none() && attr.body.is_none() && attr.query.is_none();
        let (query_names, body_names): (Vec<(String, Option<String>)>, Vec<(String, Option<String>)>) = if all_default {
            (vec![], other_args.iter().map(|(i, _)| (i.to_string(), None)).collect())
        } else {
            let qn: Vec<(String, Option<String>)> = if attr.query.is_some() {
                split_names(&attr.query)
            } else {
                other_args
                    .iter()
                    .map(|(i, _)| (i.to_string(), None))
                    .filter(|(n, _)| !path_names.iter().any(|(pn, _)| pn == n) && !body_names.iter().any(|(bn, _)| bn == n))
                    .collect()
            };
            (qn, body_names)
        };

        // 解析 method + 路径已在上面完成（rest 或默认）

        let rest_fn_name = format_ident!("{}_rest", name);
        let rest_meta_const = format_ident!("{}_REST", upper);
        let mount_fn_name = format_ident!("{}_mount", name);
        let name_str = name.to_string();
        let path_param_strs: Vec<String> = path_names.iter().map(|(n, _)| n.clone()).collect();
        // axum routing 函数名(get/post/put/delete...)
        let method_fn = format_ident!("{}", method.to_lowercase());
        let path_struct = format_ident!("{}Path", upper);
        let query_struct = format_ident!("{}Query", upper);
        let body_struct = format_ident!("{}Body", upper);

        // 各提取器结构体字段(从 other_args 取类型)
        let path_fields = fields_for(&path_names, &other_args);
        let query_fields = fields_for(&query_names, &other_args);
        let body_fields = fields_for(&body_names, &other_args);

        // 提取器参数(顺序:state 提取器(各子状态) → Path, Query, Json——body 必须最后)
        // 每个 state 参数生成一个 State<#ty> 提取器(axum FromRef<RestState> 解析)
        let rest_state_extractors: TokenStream2 = state_params
            .iter()
            .map(|(i, t)| quote! { ::axum::extract::State(#i): ::axum::extract::State<#t>, })
            .collect();
        let path_extractor = if path_names.is_empty() {
            quote! {}
        } else {
            quote! { ::axum::extract::Path(p): ::axum::extract::Path<#path_struct>, }
        };
        let query_extractor = if query_names.is_empty() {
            quote! {}
        } else {
            quote! { ::axum::extract::Query(q): ::axum::extract::Query<#query_struct>, }
        };
        let body_extractor = if body_names.is_empty() {
            quote! {}
        } else {
            quote! { ::axum::Json(b): ::axum::Json<#body_struct>, }
        };

        // 按原始签名顺序构造调用参（state 槽 → &#ident；其余按 path/body/query 取结构体字段）
        let rest_call: TokenStream2 = call_slots.iter().map(|slot| match slot {
            Slot::State(i) => { let t = i.clone(); quote! { &#t, } }
            Slot::Arg(i) => {
                let s = i.to_string();
                if path_names.iter().any(|(n, _)| n == &s) {
                    quote! { p.#i, }
                } else if body_names.iter().any(|(n, _)| n == &s) {
                    quote! { b.#i, }
                } else if query_names.iter().any(|(n, _)| n == &s) {
                    quote! { q.#i, }
                } else {
                    quote! { #i, }
                }
            }
        }).collect();

        // 返回类型处理：Result → handler 返 Result<impl IntoResponse, StatusCode>，错误 500；
        // 裸返回 → handler 直接返 impl IntoResponse（Json(_impl)，无错误路径）
        let (rest_ret_ty, rest_body): (TokenStream2, TokenStream2) = if is_result_ret {
            (quote! { std::result::Result<impl ::axum::response::IntoResponse, ::axum::http::StatusCode> },
             quote! { #impl_name(#rest_call) #await_tok .map(::axum::Json).map_err(|_| ::axum::http::StatusCode::INTERNAL_SERVER_ERROR) })
        } else {
            (quote! { impl ::axum::response::IntoResponse },
             quote! { ::axum::Json(#impl_name(#rest_call) #await_tok) })
        };
        let rest_ret_ty_def = quote! {
            /// axum handler——按 path/query/body 取参,调 `_impl`,返 JSON。
            #[cfg(feature = "rest_api")]
        };

        // 结构体定义(仅有字段的才生成;需 Deserialize)
        let path_struct_def = if path_names.is_empty() {
            quote! {}
        } else {
            quote! {
                #[cfg(feature = "rest_api")]
                #[derive(::serde::Deserialize)]
                #[doc(hidden)]
                pub struct #path_struct { #path_fields }
            }
        };
        let query_struct_def = if query_names.is_empty() {
            quote! {}
        } else {
            quote! {
                #[cfg(feature = "rest_api")]
                #[derive(::serde::Deserialize)]
                #[serde(rename_all = "camelCase")]  // 对齐 Tauri/JS camelCase（per-field as 可覆盖）
                #[doc(hidden)]
                pub struct #query_struct { #query_fields }
            }
        };
        let body_struct_def = if body_names.is_empty() {
            quote! {}
        } else {
            quote! {
                #[cfg(feature = "rest_api")]
                #[derive(::serde::Deserialize)]
                #[serde(rename_all = "camelCase")]  // 对齐 Tauri/JS camelCase 约定
                #[doc(hidden)]
                pub struct #body_struct { #body_fields }
            }
        };

        // has_state=false 时无 state 提取器(纯函数命令)
        let _ = has_state;

        Some(quote! {
            /// REST 路由元信息(method + path),供注册器收集
            #[cfg(feature = "rest_api")]
            #[doc(hidden)]
            pub const #rest_meta_const: (&str, &str) = (#method, #axum_path);

            #path_struct_def
            #query_struct_def
            #body_struct_def

            #rest_ret_ty_def
            #vis async fn #rest_fn_name(
                #rest_state_extractors
                #path_extractor
                #query_extractor
                #body_extractor
            ) -> #rest_ret_ty {
                #rest_body
            }

            /// 路由挂载器(非捕获 fn,可作 fn 指针存入注册器)
            #[cfg(feature = "rest_api")]
            #[doc(hidden)]
            pub fn #mount_fn_name(
                r: ::axum::Router<#router_ty>,
            ) -> ::axum::Router<#router_ty> {
                r.route(#axum_path, ::axum::routing::#method_fn(#rest_fn_name))
            }

            /// 自动提交到 inventory 注册器——解耦即上路由表,无 parity drift。
            #[cfg(feature = "rest_api")]
            ::inventory::submit! {
                crate::rest_registry::RouteReg {
                    method: #method,
                    path: #axum_path,
                    mount: #mount_fn_name,
                }
            }

            /// 路由元数据（供 __routes 发现端点 dump）。
            #[cfg(feature = "rest_api")]
            ::inventory::submit! {
                crate::rest_registry::RouteMeta {
                    cmd: #name_str,
                    method: #method,
                    path: #axum_path,
                    path_params: &[#(#path_param_strs),*],
                }
            }
        })
    };

    let out = quote! {
        #impl_fn
        #tauri_cmd
        #rest_handler
    };
    out.into()
}

/// 由名字列表 + 原参类型,生成结构体字段 token。
/// 名字项为 `(name, optional rename)`;有 rename 时生成 `#[serde(rename="...")]`。
fn fields_for(names: &[(String, Option<String>)], args: &[(syn::Ident, Type)]) -> TokenStream2 {
    names
        .iter()
        .filter_map(|(n, rename)| {
            args.iter()
                .find(|(i, _)| i == n)
                .map(|(i, t)| {
                    if let Some(r) = rename {
                        quote! { #[serde(rename = #r)] pub #i: #t, }
                    } else {
                        quote! { pub #i: #t, }
                    }
                })
        })
        .collect()
}

/// `"POST /control/v1/providers/{id}"` → (`"POST"`, `"/control/v1/providers/:id"`)
fn parse_rest_route(s: &str) -> (String, String) {
    let s = s.trim();
    let (method, path) = match s.split_once(' ') {
        Some((m, p)) => (m.to_string(), p.trim().to_string()),
        None => ("GET".to_string(), s.to_string()),
    };
    // {id} → :id
    let axum_path = path.replace('{', ":").replace('}', "");
    (method, axum_path)
}

/// 剥掉 `&T` / `&mut T` 的引用层,返回 T。
fn strip_ref(ty: &Type) -> Type {
    match ty {
        Type::Reference(r) => (*r.elem).clone(),
        other => other.clone(),
    }
}

/// 分析返回类型 → (是否 Result, 完整返回类型, Ok 内类型或裸类型本身)。
/// 裸返回（如 `-> bool`、`-> Vec<T>`）支持：is_result=false，ok_ty=返回类型本身。
fn analyze_return(output: &syn::ReturnType) -> (bool, Type, Type) {
    if let syn::ReturnType::Type(_, ty) = output {
        let full = (**ty).clone();
        if let Type::Path(p) = &**ty {
            if let Some(seg) = p.path.segments.last() {
                if seg.ident == "Result" {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(t)) = args.args.first() {
                            return (true, full, t.clone());
                        }
                    }
                }
            }
        }
        // 裸返回：ok_ty = 完整类型本身
        return (false, full.clone(), full);
    }
    (false, syn::parse_quote!(()), syn::parse_quote!(()))
}
