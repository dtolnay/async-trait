use crate::lifetime::CollectLifetimes;
use crate::parse::Item;
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::visit_mut::VisitMut;
use syn::{
    parse_quote, ArgCaptured, ArgSelfRef, Block, FnArg, GenericParam, Ident, ImplItem, Lifetime,
    MethodSig, Pat, ReturnType, Token, TraitItem, TypeParamBound, WhereClause,
};

impl ToTokens for Item {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Item::Trait(item) => item.to_tokens(tokens),
            Item::Impl(item) => item.to_tokens(tokens),
        }
    }
}

type Supertraits = Punctuated<TypeParamBound, Token![+]>;

pub fn expand(input: &mut Item) {
    match input {
        Item::Trait(input) => {
            for inner in &mut input.items {
                if let TraitItem::Method(method) = inner {
                    if method.sig.asyncness.is_some() {
                        if let Some(block) = &mut method.default {
                            transform_block(block);
                        }
                        let supertraits = Some(&input.supertraits);
                        let has_default = method.default.is_some();
                        transform_sig(&mut method.sig, supertraits, has_default);
                    }
                }
            }
        }
        Item::Impl(input) => {
            for inner in &mut input.items {
                if let ImplItem::Method(method) = inner {
                    if method.sig.asyncness.is_some() {
                        transform_block(&mut method.block);
                        let supertraits = None;
                        let has_default = false;
                        transform_sig(&mut method.sig, supertraits, has_default);
                    }
                }
            }
        }
    }
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret;
//
// Output:
//     fn f<'life0, 'life1, 'async_trait, T>(
//         &'life0 self,
//         x: &'life1 T,
//     ) -> Pin<Box<dyn Future<Output = Ret> + Send + 'async_trait>>
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: Sync + 'async_trait;
fn transform_sig(sig: &mut MethodSig, supertraits: Option<&Supertraits>, has_default: bool) {
    sig.decl.fn_token.span = sig.asyncness.take().unwrap().span;

    let ret = match &sig.decl.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret) => quote!(#ret),
    };

    let has_self = match sig.decl.inputs.iter_mut().next() {
        Some(FnArg::SelfRef(_)) | Some(FnArg::SelfValue(_)) => true,
        _ => false,
    };

    let mut elided = CollectLifetimes::new();
    for arg in sig.decl.inputs.iter_mut() {
        match arg {
            FnArg::SelfRef(arg) => elided.visit_arg_self_ref_mut(arg),
            FnArg::Captured(arg) => elided.visit_type_mut(&mut arg.ty),
            _ => {}
        }
    }

    let lifetime: Lifetime;
    if !sig.decl.generics.params.is_empty() || !elided.lifetimes.is_empty() || has_self {
        lifetime = parse_quote!('async_trait);
        let where_clause = sig
            .decl
            .generics
            .where_clause
            .get_or_insert_with(|| WhereClause {
                where_token: Default::default(),
                predicates: Punctuated::new(),
            });
        for param in &sig.decl.generics.params {
            match param {
                GenericParam::Type(param) => {
                    let param = &param.ident;
                    where_clause
                        .predicates
                        .push(parse_quote!(#param: #lifetime));
                }
                GenericParam::Lifetime(param) => {
                    let param = &param.lifetime;
                    where_clause
                        .predicates
                        .push(parse_quote!(#param: #lifetime));
                }
                GenericParam::Const(_) => {}
            }
        }
        for elided in elided.lifetimes {
            sig.decl.generics.params.push(parse_quote!(#elided));
            where_clause
                .predicates
                .push(parse_quote!(#elided: #lifetime));
        }
        sig.decl.generics.params.push(parse_quote!(#lifetime));
        if has_self {
            let bound: Ident = match &sig.decl.inputs[0] {
                FnArg::SelfRef(ArgSelfRef {
                    mutability: None, ..
                }) => parse_quote!(Sync),
                _ => parse_quote!(Send),
            };
            let assume_bound = match supertraits {
                Some(supertraits) => !has_default || has_bound(supertraits, &bound),
                None => true,
            };
            where_clause.predicates.push(if assume_bound {
                parse_quote!(Self: #lifetime)
            } else {
                parse_quote!(Self: std::marker::#bound + #lifetime)
            });
        }
    } else {
        lifetime = parse_quote!('static);
    };

    for (i, arg) in sig.decl.inputs.iter_mut().enumerate() {
        match arg {
            FnArg::SelfRef(_) => {}
            FnArg::SelfValue(arg) => arg.mutability = None,
            FnArg::Captured(ArgCaptured {
                pat: Pat::Ident(ident),
                ..
            }) => {
                ident.by_ref = None;
                ident.mutability = None;
            }
            FnArg::Captured(arg) => {
                let positional = positional_arg(i);
                arg.pat = parse_quote!(#positional);
            }
            FnArg::Inferred(_) | FnArg::Ignored(_) => panic!("unsupported arg"),
        }
    }

    sig.decl.output = parse_quote! {
        -> std::pin::Pin<std::boxed::Box<
            dyn std::future::Future<Output = #ret> + std::marker::Send + #lifetime
        >>
    };
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret {
//         self + x
//     }
//
// Output:
//     Pin::from(Box::new(async move {
//         _self + x
//     }))
fn transform_block(block: &mut Block) {
    let brace = block.brace_token;
    *block = parse_quote!({
        std::pin::Pin::from(std::boxed::Box::new(async move #block))
    });
    block.brace_token = brace;
}

fn positional_arg(i: usize) -> Ident {
    Ident::new(&format!("__arg{}", i), Span::call_site())
}

fn has_bound(supertraits: &Supertraits, marker: &Ident) -> bool {
    for bound in supertraits {
        if let TypeParamBound::Trait(bound) = bound {
            if bound.path.is_ident(marker.clone()) {
                return true;
            }
        }
    }
    false
}
