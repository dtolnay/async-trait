use crate::lifetime::CollectLifetimes;
use crate::parse::Item;
use crate::receiver::{has_self_in_block, has_self_in_sig};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned, ToTokens};
use syn::punctuated::Punctuated;
use syn::token::Add;
use syn::visit_mut::VisitMut;
use syn::Attribute;
use syn::{
    parse_quote, Block, FnArg, GenericParam, Generics, Ident, ImplItem, Lifetime, Pat, Receiver,
    ReturnType, Signature, Stmt, TraitItem, Type, TypeParamBound, WhereClause,
};

impl ToTokens for Item {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Item::Trait(item) => item.to_tokens(tokens),
            Item::Impl(item) => item.to_tokens(tokens),
        }
    }
}

#[derive(Clone, Copy)]
enum Context<'a> {
    Trait {
        generics: &'a Generics,
        supertraits: &'a Supertraits,
    },
    Impl {
        impl_generics: &'a Generics,
    },
}

impl Context<'_> {
    fn lifetimes<'a>(&'a self, used: &'a [Lifetime]) -> impl Iterator<Item = &'a GenericParam> {
        let generics = match self {
            Context::Trait { generics, .. } => generics,
            Context::Impl { impl_generics, .. } => impl_generics,
        };
        generics.params.iter().filter(move |param| {
            if let GenericParam::Lifetime(param) = param {
                used.contains(&param.lifetime)
            } else {
                false
            }
        })
    }
}

type Supertraits = Punctuated<TypeParamBound, Add>;

pub fn expand(input: &mut Item, is_local: bool) {
    match input {
        Item::Trait(input) => {
            let context = Context::Trait {
                generics: &input.generics,
                supertraits: &input.supertraits,
            };
            for inner in &mut input.items {
                if let TraitItem::Method(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let block = &mut method.default;
                        let mut has_self = has_self_in_sig(sig);
                        if let Some(block) = block {
                            has_self |= has_self_in_block(block);
                            transform_block(block);
                        }
                        let has_default = method.default.is_some();
                        let future_bounds = get_future_bounds(&mut method.attrs);
                        transform_sig(context, sig, has_self, has_default, is_local, future_bounds);
                        method.attrs.push(parse_quote!(#[must_use]));
                        method
                            .attrs
                            .push(parse_quote!(#[allow(clippy::needless_lifetimes)]));
                        method
                            .attrs
                            .push(parse_quote!(#[allow(clippy::extra_unused_lifetimes)]));
                        method
                            .attrs
                            .push(parse_quote!(#[allow(clippy::type_repetition_in_bounds)]));
                    }
                }
            }
        }
        Item::Impl(input) => {
            let mut lifetimes = CollectLifetimes::new("'impl");
            lifetimes.visit_type_mut(&mut *input.self_ty);
            lifetimes.visit_path_mut(&mut input.trait_.as_mut().unwrap().1);
            let params = &input.generics.params;
            let elided = lifetimes.elided;
            input.generics.params = parse_quote!(#(#elided,)* #params);

            let context = Context::Impl {
                impl_generics: &input.generics,
            };
            for inner in &mut input.items {
                if let ImplItem::Method(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let block = &mut method.block;
                        let has_self = has_self_in_sig(sig) || has_self_in_block(block);
                        let future_bounds = get_future_bounds(&mut method.attrs);
                        transform_block(block);
                        transform_sig(context, sig, has_self, false, is_local, future_bounds);
                    }
                }
            }
        }
    }
}
fn get_future_bounds(attrs: &mut Vec<Attribute>) -> FutureBounds {
    let model: Attribute = parse_quote!(#[future_is[Sync]]);
    let mut result = FutureBounds::new();
    for pos in (0..attrs.len()).rev() {
        let attr = &attrs[pos];
        if attr.path == model.path && attr.style == model.style {
            let future_is = attrs.remove(pos);
            let remaining: TokenStream = result.parse(future_is).into();
            if !remaining.is_empty() {
                attrs.insert(pos, parse_quote!(#[::async_trait::future_is(#remaining)]))
            }
        }
    }
    if !result.is_static {
        result.bounds.push(parse_quote!('async_trait));
    }
    result
}
struct FutureBounds {
    bounds: Supertraits,
    is_static: bool,
}
impl syn::parse::Parse for FutureBounds {
    fn parse(bracket: syn::parse::ParseStream) -> Result<Self, syn::Error> {
        let mut this = FutureBounds::new();
        let input;
        syn::bracketed!(input in bracket);
        while !input.is_empty() {
            let bound = input.parse()?;
            if let TypeParamBound::Lifetime(Lifetime { ref ident, .. }) = bound {
                if ident == "static" {
                    this.is_static = true;
                }
            }
            this.bounds.push(bound);
            if !input.is_empty() {
                let _add: Add = input.parse()?;
            }
        }
        Ok(this)
    }
}
impl FutureBounds {
    fn new() -> Self {
        FutureBounds {
            bounds: Supertraits::new(),
            is_static: false,
        }
    }
    fn parse(&mut self, future_is: Attribute) -> proc_macro::TokenStream {
        let stream = future_is.tokens.into();
        let result = syn::parse_macro_input!(stream as FutureBounds);
        for bound in result.bounds.into_iter() {
            self.bounds.push(bound)
        }
        if result.is_static {
            self.is_static = true;
        }
        proc_macro::TokenStream::new()
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
fn transform_sig(
    context: Context,
    sig: &mut Signature,
    has_self: bool,
    has_default: bool,
    is_local: bool,
    mut future_bounds: FutureBounds,
) {
    sig.fn_token.span = sig.asyncness.take().unwrap().span;

    let ret = match &sig.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret) => quote!(#ret),
    };

    let mut lifetimes = CollectLifetimes::new("'life");
    if !future_bounds.is_static {
        for arg in sig.inputs.iter_mut() {
            match arg {
                FnArg::Receiver(arg) => lifetimes.visit_receiver_mut(arg),
                FnArg::Typed(arg) => lifetimes.visit_type_mut(&mut arg.ty),
            }
        }
    }

    let where_clause = sig
        .generics
        .where_clause
        .get_or_insert_with(|| WhereClause {
            where_token: Default::default(),
            predicates: Punctuated::new(),
        });
    for param in sig
        .generics
        .params
        .iter()
        .chain(context.lifetimes(&lifetimes.explicit))
    {
        match param {
            GenericParam::Type(param) => {
                if !future_bounds.is_static {
                    let param = &param.ident;
                    where_clause
                        .predicates
                        .push(parse_quote!(#param: 'async_trait));
                }
            }
            GenericParam::Lifetime(param) => {
                if !future_bounds.is_static {
                    let param = &param.lifetime;
                    where_clause
                        .predicates
                        .push(parse_quote!(#param: 'async_trait));
                }
            }
            GenericParam::Const(_) => {}
        }
    }
    if !future_bounds.is_static {
        for elided in lifetimes.elided {
            sig.generics.params.push(parse_quote!(#elided));
            where_clause
                .predicates
                .push(parse_quote!(#elided: 'async_trait));
        }
    }
    sig.generics.params.push(parse_quote!('async_trait));
    if has_self {
        let bound: Ident = match sig.inputs.iter().next() {
            Some(FnArg::Receiver(Receiver {
                reference: Some(_),
                mutability: None,
                ..
            })) => parse_quote!(Sync),
            Some(FnArg::Typed(arg))
                if match (arg.pat.as_ref(), arg.ty.as_ref()) {
                    (Pat::Ident(pat), Type::Reference(ty)) => {
                        pat.ident == "self" && ty.mutability.is_none()
                    }
                    _ => false,
                } =>
            {
                parse_quote!(Sync)
            }
            _ => parse_quote!(Send),
        };
        let assume_bound = match context {
            Context::Trait { supertraits, .. } => !has_default || has_bound(supertraits, &bound),
            Context::Impl { .. } => true,
        };
        if !future_bounds.is_static {
            where_clause
                .predicates
                .push(parse_quote!(Self: 'async_trait));
        }
        if !(assume_bound || is_local) {
            where_clause
                .predicates
                .push(parse_quote!(Self: ::core::marker::#bound ));
        }
    }

    for (i, arg) in sig.inputs.iter_mut().enumerate() {
        match arg {
            FnArg::Receiver(_) => {}
            FnArg::Typed(arg) => {
                if let Pat::Ident(ident) = &mut *arg.pat {
                    ident.by_ref = None;
                } else {
                    let positional = positional_arg(i);
                    *arg.pat = parse_quote!(#positional);
                }
            }
        }
    }

    if !is_local {
        future_bounds
            .bounds
            .push(parse_quote!(::core::marker::Send));
    }
    let bounds = future_bounds.bounds;
    sig.output = parse_quote! {
        -> ::core::pin::Pin<::std::boxed::Box<
            dyn ::core::future::Future<Output = #ret> + #bounds
        >>
    };
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret {
//         self + x
//     }
//
// Output:
//     let fut = async move {
//         self + x
//     };
//     Box::new(fut)
fn transform_block(block: &mut Block) {
    if let Some(Stmt::Item(syn::Item::Verbatim(item))) = block.stmts.first() {
        if block.stmts.len() == 1 && item.to_string() == ";" {
            return;
        }
    }
    let brace = block.brace_token;
    let boxed = quote_spanned!(brace.span=> {
        let fut = async move { #block };
        ::std::boxed::Box::pin(fut)
    });
    *block = parse_quote!(#boxed);
    block.brace_token = brace;
}

fn positional_arg(i: usize) -> Ident {
    format_ident!("__arg{}", i)
}

fn has_bound(supertraits: &Supertraits, marker: &Ident) -> bool {
    for bound in supertraits {
        if let TypeParamBound::Trait(bound) = bound {
            if bound.path.is_ident(marker) {
                return true;
            }
        }
    }
    false
}
