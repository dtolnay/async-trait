use crate::bound::{has_bound, InferredBound, Supertraits};
use crate::lifetime::{AddLifetimeToImplTrait, CollectLifetimes};
use crate::parse::Item;
use crate::receiver::{has_self_in_block, has_self_in_sig, mut_pat, ReplaceSelf};
use crate::verbatim::VerbatimFn;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use std::collections::BTreeSet as Set;
use std::mem;
use syn::punctuated::{Pair, Punctuated};
use syn::visit_mut::{self, VisitMut};
use syn::{
    parse_quote, parse_quote_spanned, Attribute, Block, FnArg, GenericArgument, GenericParam,
    Generics, Ident, ImplItem, ImplItemType, Lifetime, LifetimeParam, Pat, PatIdent, PathArguments,
    Receiver, ReturnType, Signature, Token, TraitItem, TraitItemType, Type, TypePath, WhereClause,
};

impl ToTokens for Item {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Item::Trait(item) => item.to_tokens(tokens),
            Item::Impl(item) => item.to_tokens(tokens),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FutureType {
    /// The returned `Future` is boxed and can be sent.
    BoxedSend,

    /// The returned `Future` is boxed and cannot be sent.
    BoxedLocal,

    /// The returned `Future` is an `impl Future` and can be sent.
    UnboxedSend,

    /// The returned `Future` is an `impl Future` and cannot be sent.
    UnboxedLocal,

    /// The returned `Future` is an `impl Future` with the same lifetime bound for all the
    /// references and the `Future`.
    UnboxedSimpleSend,

    /// The returned `Future` is an `impl Future` with the same lifetime bound for all the
    /// references and the `Future`. The future cannot be sent.
    UnboxedSimpleLocal,
}

impl FutureType {
    fn is_boxed(self) -> bool {
        match self {
            FutureType::BoxedSend | FutureType::BoxedLocal => true,
            FutureType::UnboxedSend
            | FutureType::UnboxedLocal
            | FutureType::UnboxedSimpleSend
            | FutureType::UnboxedSimpleLocal => false,
        }
    }

    fn is_send(self) -> bool {
        match self {
            FutureType::BoxedSend | FutureType::UnboxedSend | FutureType::UnboxedSimpleSend => true,
            FutureType::BoxedLocal | FutureType::UnboxedLocal | FutureType::UnboxedSimpleLocal => {
                false
            }
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
        associated_type_impl_traits: &'a Set<Ident>,
    },
}

impl Context<'_> {
    fn lifetimes<'a>(&'a self, used: &'a [Lifetime]) -> impl Iterator<Item = &'a LifetimeParam> {
        let generics = match self {
            Context::Trait { generics, .. } => generics,
            Context::Impl { impl_generics, .. } => impl_generics,
        };
        generics.params.iter().filter_map(move |param| {
            if let GenericParam::Lifetime(param) = param {
                if used.contains(&param.lifetime) {
                    return Some(param);
                }
            }
            None
        })
    }
}

pub fn expand(input: &mut Item, is_local: bool) {
    match input {
        Item::Trait(input) => {
            let mut implicit_associated_types: Vec<TraitItem> = Vec::new();
            let context = Context::Trait {
                generics: &input.generics,
                supertraits: &input.supertraits,
            };
            for inner in &mut input.items {
                if let TraitItem::Fn(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let future_type = future_type_attr(&method.attrs, is_local);
                        let ret = ret_token_stream(&sig.output);
                        let block = &mut method.default;
                        let mut has_self = has_self_in_sig(sig);
                        method.attrs.push(parse_quote!(#[must_use]));
                        if let Some(block) = block {
                            has_self |= has_self_in_block(block);
                            transform_block(context, sig, block, future_type);
                            method.attrs.push(lint_suppress_with_body());
                        } else {
                            method.attrs.push(lint_suppress_without_body());
                        }
                        let has_default = method.default.is_some();
                        let bounds =
                            transform_sig(context, sig, has_self, has_default, future_type);
                        if !future_type.is_boxed() {
                            let type_def = define_implicit_associated_type(sig, &ret, &bounds);
                            implicit_associated_types.push(TraitItem::Type(type_def));
                            generate_fn_doc(sig, &ret, &mut method.attrs);
                        }
                    }
                }
            }
            implicit_associated_types
                .into_iter()
                .for_each(|t| input.items.push(t));
        }
        Item::Impl(input) => {
            let mut associated_type_impl_traits = Set::new();
            for inner in &input.items {
                if let ImplItem::Type(assoc) = inner {
                    if let Type::ImplTrait(_) = assoc.ty {
                        associated_type_impl_traits.insert(assoc.ident.clone());
                    }
                }
            }

            let mut implicit_associated_type_assigns: Vec<ImplItem> = Vec::new();
            let context = Context::Impl {
                impl_generics: &input.generics,
                associated_type_impl_traits: &associated_type_impl_traits,
            };
            for inner in &mut input.items {
                match inner {
                    ImplItem::Fn(method) if method.sig.asyncness.is_some() => {
                        let future_type = future_type_attr(&method.attrs, is_local);
                        let sig = &mut method.sig;
                        let ret = ret_token_stream(&sig.output);
                        let block = &mut method.block;
                        let has_self = has_self_in_sig(sig) || has_self_in_block(block);
                        transform_block(context, sig, block, future_type);
                        let bounds = transform_sig(context, sig, has_self, false, future_type);
                        if !future_type.is_boxed() {
                            let type_assign = assign_implicit_associated_type(sig, &ret, &bounds);
                            implicit_associated_type_assigns.push(ImplItem::Type(type_assign));
                            generate_fn_doc(sig, &ret, &mut method.attrs);
                        }
                        method.attrs.push(lint_suppress_with_body());
                    }
                    ImplItem::Verbatim(tokens) => {
                        let mut method = match syn::parse2::<VerbatimFn>(tokens.clone()) {
                            Ok(method) if method.sig.asyncness.is_some() => method,
                            _ => continue,
                        };
                        let future_type = future_type_attr(&method.attrs, is_local);
                        let sig = &mut method.sig;
                        let has_self = has_self_in_sig(sig);
                        transform_sig(context, sig, has_self, false, future_type);
                        method.attrs.push(lint_suppress_with_body());
                        *tokens = quote!(#method);
                    }
                    _ => {}
                }
            }
            implicit_associated_type_assigns
                .into_iter()
                .for_each(|t| input.items.push(t));
        }
    }
}

fn lint_suppress_with_body() -> Attribute {
    parse_quote! {
        #[allow(
            clippy::async_yields_async,
            clippy::diverging_sub_expression,
            clippy::let_unit_value,
            clippy::no_effect_underscore_binding,
            clippy::shadow_same,
            clippy::type_complexity,
            clippy::type_repetition_in_bounds,
            clippy::used_underscore_binding
        )]
    }
}

fn lint_suppress_without_body() -> Attribute {
    parse_quote! {
        #[allow(
            clippy::type_complexity,
            clippy::type_repetition_in_bounds
        )]
    }
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret;
//
// Output (future_type == Boxed):
//     fn f<'life0, 'life1, 'async_trait, T>(
//         &'life0 self,
//         x: &'life1 T,
//     ) -> Pin<Box<dyn Future<Output = Ret> + Send + 'async_trait>>
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: Sync + 'async_trait;
//
// Output (future_type == Unboxed):
//     fn f<'life0, 'life1, 'async_trait, T>(
//         &'life0 self,
//         x: &'life1 T,
//     ) -> Self::RetTypeOfF<'life0, 'life1, 'async_trait>
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: 'async_trait;
//
// Output (future_type == UnboxedSimple):
//     fn f<'async_trait, T>(
//         &'async_trait self,
//         x: &'async_trait T,
//     ) -> Self::RetTypeOfF<'async_trait>
//     where
//         T: 'async_trait,
//         Self: 'async_trait;
fn transform_sig(
    context: Context,
    sig: &mut Signature,
    has_self: bool,
    has_default: bool,
    future_type: FutureType,
) -> TokenStream {
    let default_span = sig.asyncness.take().unwrap().span;
    sig.fn_token.span = default_span;

    let (ret_arrow, ret) = match &sig.output {
        ReturnType::Default => (Token![->](default_span), quote_spanned!(default_span=> ())),
        ReturnType::Type(arrow, ret) => (*arrow, quote!(#ret)),
    };

    let mut lifetimes = CollectLifetimes::new();
    for arg in &mut sig.inputs {
        match arg {
            FnArg::Receiver(arg) => {
                lifetimes.visit_receiver_mut(arg);
                if future_type == FutureType::UnboxedSimpleSend {
                    if let Some(arg_ref) = arg.reference.as_mut() {
                        arg_ref
                            .1
                            .replace(Lifetime::new("'async_trait", Span::call_site()));
                    }
                }
            }
            FnArg::Typed(arg) => {
                lifetimes.visit_type_mut(&mut arg.ty);
                if future_type == FutureType::UnboxedSend {
                    if let Type::Reference(arg_ty) = &*arg.ty {
                        if let Some(lifetime) = arg_ty.lifetime.as_ref() {
                            let span = lifetime.span();
                            let ty = &*arg_ty.elem;
                            where_clause_or_default(&mut sig.generics.where_clause)
                                .predicates
                                .push(parse_quote_spanned!(span=> #ty: #lifetime));
                        }
                    }
                } else if future_type == FutureType::UnboxedSimpleSend {
                    if let Type::Reference(arg_ty) = &mut *arg.ty {
                        arg_ty
                            .lifetime
                            .replace(Lifetime::new("'async_trait", Span::call_site()));
                    }
                }
            }
        }
    }

    if future_type == FutureType::UnboxedSimpleSend {
        let mut lifetime_replaced = false;
        let mut filtered_params = Vec::<Pair<GenericParam, Token![,]>>::new();
        while let Some(param) = sig.generics.params.pop() {
            if let Pair::Punctuated(param, token) = param {
                match param {
                    GenericParam::Lifetime(mut lifetime) => {
                        if !lifetime_replaced {
                            lifetime.lifetime = Lifetime::new("'async_trait", Span::call_site());
                            lifetime.bounds.clear();
                            filtered_params
                                .push(Pair::Punctuated(GenericParam::Lifetime(lifetime), token));
                            lifetime_replaced = true;
                        }
                    }
                    param => filtered_params.push(Pair::Punctuated(param, token)),
                }
            }
        }
        if !lifetime_replaced {
            sig.generics
                .params
                .push(parse_quote_spanned!(default_span=> 'async_trait));
        }
        while let Some(param) = filtered_params.pop() {
            sig.generics.params.push(param.into_value());
        }
    } else {
        for param in &mut sig.generics.params {
            match param {
                GenericParam::Type(param) => {
                    let param_name = &param.ident;
                    let span = match param.colon_token.take() {
                        Some(colon_token) => colon_token.span,
                        None => param_name.span(),
                    };
                    let bounds = mem::replace(&mut param.bounds, Punctuated::new());
                    where_clause_or_default(&mut sig.generics.where_clause)
                        .predicates
                        .push(parse_quote_spanned!(span=> #param_name: 'async_trait + #bounds));
                }
                GenericParam::Lifetime(param) => {
                    let param_name = &param.lifetime;
                    let span = match param.colon_token.take() {
                        Some(colon_token) => colon_token.span,
                        None => param_name.span(),
                    };
                    let bounds = mem::replace(&mut param.bounds, Punctuated::new());
                    where_clause_or_default(&mut sig.generics.where_clause)
                        .predicates
                        .push(parse_quote_spanned!(span=> #param: 'async_trait + #bounds));
                }
                GenericParam::Const(_) => {}
            }
        }

        for param in context.lifetimes(&lifetimes.explicit) {
            let param = &param.lifetime;
            let span = param.span();
            where_clause_or_default(&mut sig.generics.where_clause)
                .predicates
                .push(parse_quote_spanned!(span=> #param: 'async_trait));
        }
    }

    if sig.generics.lt_token.is_none() {
        sig.generics.lt_token = Some(Token![<](sig.ident.span()));
    }
    if sig.generics.gt_token.is_none() {
        sig.generics.gt_token = Some(Token![>](sig.paren_token.span.join()));
    }

    if future_type != FutureType::UnboxedSimpleSend {
        for elided in lifetimes.elided {
            sig.generics.params.push(parse_quote!(#elided));
            where_clause_or_default(&mut sig.generics.where_clause)
                .predicates
                .push(parse_quote_spanned!(elided.span()=> #elided: 'async_trait));
        }
        sig.generics
            .params
            .push(parse_quote_spanned!(default_span=> 'async_trait));
    }

    if has_self {
        let bounds: &[InferredBound] = if let Some(receiver) = sig.receiver() {
            match receiver.ty.as_ref() {
                // self: &Self
                Type::Reference(ty) if ty.mutability.is_none() => &[InferredBound::Sync],
                // self: Arc<Self>
                Type::Path(ty)
                    if {
                        let segment = ty.path.segments.last().unwrap();
                        segment.ident == "Arc"
                            && match &segment.arguments {
                                PathArguments::AngleBracketed(arguments) => {
                                    arguments.args.len() == 1
                                        && match &arguments.args[0] {
                                            GenericArgument::Type(Type::Path(arg)) => {
                                                arg.path.is_ident("Self")
                                            }
                                            _ => false,
                                        }
                                }
                                _ => false,
                            }
                    } =>
                {
                    &[InferredBound::Sync, InferredBound::Send]
                }
                _ => &[InferredBound::Send],
            }
        } else {
            &[InferredBound::Send]
        };

        let bounds = bounds.iter().filter_map(|bound| {
            let assume_bound = match context {
                Context::Trait { supertraits, .. } => !has_default || has_bound(supertraits, bound),
                Context::Impl { .. } => true,
            };
            if assume_bound || !future_type.is_send() {
                None
            } else {
                Some(bound.spanned_path(default_span))
            }
        });

        where_clause_or_default(&mut sig.generics.where_clause)
            .predicates
            .push(parse_quote_spanned! {default_span=>
                Self: #(#bounds +)* 'async_trait
            });
    }

    for (i, arg) in sig.inputs.iter_mut().enumerate() {
        match arg {
            FnArg::Receiver(receiver) => {
                if receiver.reference.is_none() {
                    receiver.mutability = None;
                }
            }
            FnArg::Typed(arg) => {
                if match *arg.ty {
                    Type::Reference(_) => false,
                    _ => true,
                } {
                    if let Pat::Ident(pat) = &mut *arg.pat {
                        pat.by_ref = None;
                        pat.mutability = None;
                    } else {
                        let positional = positional_arg(i, &arg.pat);
                        let m = mut_pat(&mut arg.pat);
                        arg.pat = parse_quote!(#m #positional);
                    }
                }
                AddLifetimeToImplTrait.visit_type_mut(&mut arg.ty);
            }
        }
    }

    let bounds = if future_type.is_send() {
        quote_spanned!(default_span=> ::core::marker::Send + 'async_trait)
    } else {
        quote_spanned!(default_span=> 'async_trait)
    };

    if future_type.is_boxed() {
        sig.output = parse_quote_spanned! {default_span=>
            #ret_arrow ::core::pin::Pin<Box<
                dyn ::core::future::Future<Output = #ret> + #bounds
            >>
        };
    } else {
        let implicit_type_name = derive_implicit_type_name(&sig.ident);
        let params_clone = sig.generics.params.clone();
        let params_iter = params_clone.into_iter().map(|mut p| {
            match &mut p {
                GenericParam::Type(t) => {
                    t.attrs.clear();
                    t.bounds.clear();
                }
                GenericParam::Lifetime(l) => {
                    l.attrs.clear();
                    l.bounds.clear();
                }
                GenericParam::Const(_) => (),
            };
            p
        });
        sig.output = parse_quote_spanned! {default_span=>
            #ret_arrow Self::#implicit_type_name<#(#params_iter),*>
        };
    }

    bounds
}

// Input:
//     async fn f<T>(&self, x: &T, (a, b): (A, B)) -> Ret {
//         self + x + a + b
//     }
//
// Output (future_type == Boxed):
//     Box::pin(async move {
//         let ___ret: Ret = {
//             let __self = self;
//             let x = x;
//             let (a, b) = __arg1;
//
//             __self + x + a + b
//         };
//
//         ___ret
//     })
//
// Output (future_type == Unboxed || future_type == UnboxedSimple):
//     async move {
//         let __ret: Ret = {
//             let __self = self;
//             let x = x;
//             let (a, b) = __arg1;
//
//             __self + x + a + b
//         };
//
//         __ret
//     }
fn transform_block(
    context: Context,
    sig: &mut Signature,
    block: &mut Block,
    future_type: FutureType,
) {
    let mut self_span = None;
    let decls = sig
        .inputs
        .iter()
        .enumerate()
        .map(|(i, arg)| match arg {
            FnArg::Receiver(Receiver {
                self_token,
                mutability,
                ..
            }) => {
                let ident = Ident::new("__self", self_token.span);
                self_span = Some(self_token.span);
                quote!(let #mutability #ident = #self_token;)
            }
            FnArg::Typed(arg) => {
                // If there is a #[cfg(...)] attribute that selectively enables
                // the parameter, forward it to the variable.
                //
                // This is currently not applied to the `self` parameter.
                let attrs = arg.attrs.iter().filter(|attr| attr.path().is_ident("cfg"));

                if let Type::Reference(_) = *arg.ty {
                    quote!()
                } else if let Pat::Ident(PatIdent {
                    ident, mutability, ..
                }) = &*arg.pat
                {
                    quote! {
                        #(#attrs)*
                        let #mutability #ident = #ident;
                    }
                } else {
                    let pat = &arg.pat;
                    let ident = positional_arg(i, pat);
                    if let Pat::Wild(_) = **pat {
                        quote! {
                            #(#attrs)*
                            let #ident = #ident;
                        }
                    } else {
                        quote! {
                            #(#attrs)*
                            let #pat = {
                                let #ident = #ident;
                                #ident
                            };
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    if let Some(span) = self_span {
        let mut replace_self = ReplaceSelf(span);
        replace_self.visit_block_mut(block);
    }

    let stmts = &block.stmts;
    let let_ret = match &mut sig.output {
        ReturnType::Default => quote_spanned! {block.brace_token.span=>
            #(#decls)*
            let () = { #(#stmts)* };
        },
        ReturnType::Type(_, ret) => {
            if contains_associated_type_impl_trait(context, ret) {
                if decls.is_empty() {
                    quote!(#(#stmts)*)
                } else {
                    quote!(#(#decls)* { #(#stmts)* })
                }
            } else {
                quote_spanned! {block.brace_token.span=>
                    if let ::core::option::Option::Some(__ret) = ::core::option::Option::None::<#ret> {
                        return __ret;
                    }
                    #(#decls)*
                    let __ret: #ret = { #(#stmts)* };
                    #[allow(unreachable_code)]
                    __ret
                }
            }
        }
    };

    if future_type.is_boxed() {
        let box_pin = quote_spanned!(block.brace_token.span=>
            Box::pin(async move { #let_ret })
        );
        block.stmts = parse_quote!(#box_pin);
    } else {
        let async_block = quote_spanned!(block.brace_token.span=>
            async move { #let_ret }
        );
        block.stmts = parse_quote!(#async_block);
    }
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret;
//
// Output:
//     type RetTypeOfF<'life0, 'life1, 'async_trait, T>: Future<Output = Ret> + Send + 'async_trait
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: 'life0;
fn define_implicit_associated_type(
    sig: &Signature,
    ret: &TokenStream,
    bounds: &TokenStream,
) -> TraitItemType {
    let implicit_type_name = derive_implicit_type_name(&sig.ident);
    let generated_doc = format!(
        "Automatically generated return type placeholder for [`Self::{}`]",
        sig.ident
    );
    let mut implicit_type_def: TraitItemType = parse_quote!(
        #[allow(clippy::needless_lifetimes, clippy::type_repetition_in_bounds)]
        #[doc = #generated_doc]
        type #implicit_type_name: ::core::future::Future<Output = #ret> + #bounds;
    );
    implicit_type_def.generics = sig.generics.clone();
    if let Some(receiver_lifetime) = receiver_lifetime(sig) {
        where_clause_or_default(&mut implicit_type_def.generics.where_clause)
            .predicates
            .push(parse_quote!(Self: #receiver_lifetime));
    }
    implicit_type_def
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret;
//
// Output:
//     type RetTypeOfF<'life0, 'life1, 'async_trait, T>
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: 'life0
//     = impl Future<Output = Ret> + Send + 'async_trait;
fn assign_implicit_associated_type(
    sig: &Signature,
    ret: &TokenStream,
    bounds: &TokenStream,
) -> ImplItemType {
    let implicit_type_name = derive_implicit_type_name(&sig.ident);
    let generated_doc = format!(
        "Automatically generated return type for [`Self::{}`]",
        sig.ident
    );
    let mut implicit_type_assign: ImplItemType = parse_quote!(
        #[allow(clippy::needless_lifetimes, clippy::type_repetition_in_bounds)]
        #[doc = #generated_doc]
        type #implicit_type_name = impl ::core::future::Future<Output = #ret> + #bounds;
    );
    implicit_type_assign.generics = sig.generics.clone();
    if let Some(receiver_lifetime) = receiver_lifetime(sig) {
        where_clause_or_default(&mut implicit_type_assign.generics.where_clause)
            .predicates
            .push(parse_quote!(Self: #receiver_lifetime));
    }
    implicit_type_assign
}

// Input:
//     /// Doc.
//     async fn f<T>(&self, x: &T) -> Ret;
//
// Output:
//     /// Doc.
//     ///
//     /// ***
//     /// _This is an asynchronous method returning [`impl Future<Output = Ret>`](Self::RetTypeOfF)._
//     async fn f<T>(&self, x: &T) -> Ret;
fn generate_fn_doc(sig: &Signature, ret: &TokenStream, attrs: &mut Vec<Attribute>) {
    let newline = quote! {
        #[doc = ""]
    };
    attrs.push(parse_quote!(#newline));
    let separator = quote! {
        #[doc = "***"]
    };
    attrs.push(parse_quote!(#separator));
    let implicit_type_name = derive_implicit_type_name(&sig.ident);
    let doc = format!(
        "_This is an asynchronous method returning [`impl Future<Output = {}>`](Self::{})._",
        ret, implicit_type_name
    );
    let doc_token_stream = quote! {
        #[doc = #doc]
    };
    attrs.push(parse_quote!(#doc_token_stream));
}

fn positional_arg(i: usize, pat: &Pat) -> Ident {
    let span: Span = syn::spanned::Spanned::span(pat);
    #[cfg(not(no_span_mixed_site))]
    let span = span.resolved_at(Span::mixed_site());
    format_ident!("__arg{}", i, span = span)
}

fn contains_associated_type_impl_trait(context: Context, ret: &mut Type) -> bool {
    struct AssociatedTypeImplTraits<'a> {
        set: &'a Set<Ident>,
        contains: bool,
    }

    impl<'a> VisitMut for AssociatedTypeImplTraits<'a> {
        fn visit_type_path_mut(&mut self, ty: &mut TypePath) {
            if ty.qself.is_none()
                && ty.path.segments.len() == 2
                && ty.path.segments[0].ident == "Self"
                && self.set.contains(&ty.path.segments[1].ident)
            {
                self.contains = true;
            }
            visit_mut::visit_type_path_mut(self, ty);
        }
    }

    match context {
        Context::Trait { .. } => false,
        Context::Impl {
            associated_type_impl_traits,
            ..
        } => {
            let mut visit = AssociatedTypeImplTraits {
                set: associated_type_impl_traits,
                contains: false,
            };
            visit.visit_type_mut(ret);
            visit.contains
        }
    }
}

/// Determines the type of the `Future`.
///
/// If none found, the `Future` is boxed.
fn future_type_attr(attrs: &[Attribute], is_local: bool) -> FutureType {
    for attr in attrs {
        if let Some(last_seg) = attr.path().segments.last() {
            let future_type = match (last_seg.ident.to_string().as_str(), is_local) {
                ("unboxed", false) => Some(FutureType::UnboxedSend),
                ("unboxed", true) => Some(FutureType::UnboxedLocal),
                ("unboxed_simple", false) => Some(FutureType::UnboxedSimpleSend),
                ("unboxed_simple", true) => Some(FutureType::UnboxedSimpleLocal),
                _ => None,
            };
            if let Some(future_type) = future_type {
                return future_type;
            }
        }
    }
    if is_local {
        FutureType::BoxedLocal
    } else {
        FutureType::BoxedSend
    }
}

fn where_clause_or_default(clause: &mut Option<WhereClause>) -> &mut WhereClause {
    clause.get_or_insert_with(|| WhereClause {
        where_token: Default::default(),
        predicates: Punctuated::new(),
    })
}

fn derive_implicit_type_name(id: &Ident) -> Ident {
    let mut to_upper = true;
    let upper_camel_case_id: String = id
        .to_string()
        .chars()
        .filter_map(|c| {
            if c == '_' {
                to_upper = true;
                None
            } else if to_upper {
                to_upper = false;
                c.to_uppercase().next()
            } else {
                Some(c)
            }
        })
        .collect();
    format_ident!("RetTypeOf{}", upper_camel_case_id)
}

fn receiver_lifetime(sig: &Signature) -> Option<Lifetime> {
    for arg in &sig.inputs {
        if let FnArg::Receiver(arg) = arg {
            if let Some((_, lifetime)) = &arg.reference {
                return lifetime.as_ref().cloned();
            }
        }
    }
    None
}

fn ret_token_stream(ret_type: &ReturnType) -> TokenStream {
    match &ret_type {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret) => quote!(#ret),
    }
}
