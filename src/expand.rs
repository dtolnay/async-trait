use crate::lifetime::CollectLifetimes;
use crate::parse::Item;
use crate::receiver::{has_self_in_block, has_self_in_sig, mut_pat, ReplaceSelf};
use convert_case::{Case, Casing};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned, ToTokens};
use std::collections::BTreeSet as Set;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{
    parse_quote, parse_quote_spanned, Attribute, Block, FnArg, GenericParam, Generics, Ident,
    ImplItem, ImplItemType, Lifetime, Pat, PatIdent, Receiver, ReturnType, Signature, Stmt, Token,
    TraitItem, TraitItemType, Type, TypeParamBound, TypePath, WhereClause,
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
        associated_type_impl_traits: &'a Set<Ident>,
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

type Supertraits = Punctuated<TypeParamBound, Token![+]>;

pub fn expand(input: &mut Item, is_local: bool) {
    match input {
        Item::Trait(input) => {
            let mut implicit_associated_types: Vec<TraitItem> = Vec::new();
            let context = Context::Trait {
                generics: &input.generics,
                supertraits: &input.supertraits,
            };
            for inner in &mut input.items {
                if let TraitItem::Method(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let static_ret = contains_static_future_attr(&method.attrs);
                        let ret = ret_token_stream(&sig.output);
                        let block = &mut method.default;
                        let mut has_self = has_self_in_sig(sig);
                        method.attrs.push(parse_quote!(#[must_use]));
                        if let Some(block) = block {
                            has_self |= has_self_in_block(block);
                            transform_block(context, sig, block, static_ret);
                            method.attrs.push(lint_suppress_with_body());
                        } else {
                            method.attrs.push(lint_suppress_without_body());
                        }
                        let has_default = method.default.is_some();
                        transform_sig(
                            context,
                            sig,
                            &ret,
                            has_self,
                            has_default,
                            is_local,
                            static_ret,
                        );
                        if static_ret {
                            let type_def = define_implicit_associated_type(sig, &ret, is_local);
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
            let mut lifetimes = CollectLifetimes::new("'impl", input.impl_token.span);
            lifetimes.visit_type_mut(&mut *input.self_ty);
            lifetimes.visit_path_mut(&mut input.trait_.as_mut().unwrap().1);
            let params = &input.generics.params;
            let elided = lifetimes.elided;
            input.generics.params = parse_quote!(#(#elided,)* #params);

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
                if let ImplItem::Method(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let static_ret = contains_static_future_attr(&method.attrs);
                        let ret = ret_token_stream(&sig.output);
                        let block = &mut method.block;
                        let has_self = has_self_in_sig(sig) || has_self_in_block(block);
                        transform_block(context, sig, block, static_ret);
                        transform_sig(context, sig, &ret, has_self, false, is_local, static_ret);
                        if static_ret {
                            let type_assign = assign_implicit_associated_type(sig, &ret);
                            implicit_associated_type_assigns.push(ImplItem::Type(type_assign));
                            generate_fn_doc(sig, &ret, &mut method.attrs);
                        }
                        method.attrs.push(lint_suppress_with_body());
                    }
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
// Output (static_future == false):
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
// Output (static_future == true):
//     fn f<'life0, 'life1, 'async_trait, T>(
//         &'life0 self,
//         x: &'life1 T,
//     ) -> Self::RetTypeOfF<'_>
//     where
//         'life0: 'async_trait,
//         'life1: 'async_trait,
//         T: 'async_trait,
//         Self: Sync + 'async_trait;
fn transform_sig(
    context: Context,
    sig: &mut Signature,
    ret: &TokenStream,
    has_self: bool,
    has_default: bool,
    is_local: bool,
    static_future: bool,
) {
    sig.fn_token.span = sig.asyncness.take().unwrap().span;

    let default_span = sig
        .ident
        .span()
        .join(sig.paren_token.span)
        .unwrap_or_else(|| sig.ident.span());

    let mut lifetimes = CollectLifetimes::new("'life", default_span);
    for arg in sig.inputs.iter_mut() {
        match arg {
            FnArg::Receiver(arg) => lifetimes.visit_receiver_mut(arg),
            FnArg::Typed(arg) => {
                lifetimes.visit_type_mut(&mut arg.ty);
                if static_future {
                    if let Type::Reference(ref_ty) = &*arg.ty {
                        if let Some(lifetime) = ref_ty.lifetime.as_ref() {
                            let span = lifetime.span();
                            where_clause_or_default(&mut sig.generics.where_clause)
                                .predicates
                                .push(parse_quote_spanned!(span=> #ref_ty: #lifetime));
                        }
                    }
                }
            }
        }
    }

    for param in sig
        .generics
        .params
        .iter()
        .chain(context.lifetimes(&lifetimes.explicit))
    {
        match param {
            GenericParam::Type(param) => {
                let param = &param.ident;
                let span = param.span();
                where_clause_or_default(&mut sig.generics.where_clause)
                    .predicates
                    .push(parse_quote_spanned!(span=> #param: 'async_trait));
            }
            GenericParam::Lifetime(param) => {
                let param = &param.lifetime;
                let span = param.span();
                where_clause_or_default(&mut sig.generics.where_clause)
                    .predicates
                    .push(parse_quote_spanned!(span=> #param: 'async_trait));
            }
            GenericParam::Const(_) => {}
        }
    }

    if sig.generics.lt_token.is_none() {
        sig.generics.lt_token = Some(Token![<](sig.ident.span()));
    }
    if sig.generics.gt_token.is_none() {
        sig.generics.gt_token = Some(Token![>](sig.paren_token.span));
    }

    for elided in lifetimes.elided {
        sig.generics.params.push(parse_quote!(#elided));
        where_clause_or_default(&mut sig.generics.where_clause)
            .predicates
            .push(parse_quote_spanned!(elided.span()=> #elided: 'async_trait));
    }

    sig.generics
        .params
        .push(parse_quote_spanned!(default_span=> 'async_trait));

    if has_self {
        let bound_span = sig.ident.span();
        let bound = match sig.inputs.iter().next() {
            Some(FnArg::Receiver(Receiver {
                reference: Some(_),
                mutability: None,
                ..
            })) => Ident::new("Sync", bound_span),
            Some(FnArg::Typed(arg))
                if match (arg.pat.as_ref(), arg.ty.as_ref()) {
                    (Pat::Ident(pat), Type::Reference(ty)) => {
                        pat.ident == "self" && ty.mutability.is_none()
                    }
                    _ => false,
                } =>
            {
                Ident::new("Sync", bound_span)
            }
            _ => Ident::new("Send", bound_span),
        };

        let assume_bound = match context {
            Context::Trait { supertraits, .. } => !has_default || has_bound(supertraits, &bound),
            Context::Impl { .. } => true,
        };

        let where_clause = where_clause_or_default(&mut sig.generics.where_clause);
        where_clause.predicates.push(if assume_bound || is_local {
            parse_quote_spanned!(bound_span=> Self: 'async_trait)
        } else {
            parse_quote_spanned!(bound_span=> Self: ::core::marker::#bound + 'async_trait)
        });
    }

    for (i, arg) in sig.inputs.iter_mut().enumerate() {
        match arg {
            FnArg::Receiver(Receiver {
                reference: Some(_), ..
            }) => {}
            FnArg::Receiver(arg) => arg.mutability = None,
            FnArg::Typed(arg) => {
                if let Pat::Ident(ident) = &mut *arg.pat {
                    ident.by_ref = None;
                    ident.mutability = None;
                } else {
                    let positional = positional_arg(i, &arg.pat);
                    let m = mut_pat(&mut arg.pat);
                    arg.pat = parse_quote!(#m #positional);
                }
            }
        }
    }

    let ret_span = sig.ident.span();
    let bounds = if is_local {
        quote_spanned!(ret_span=> 'async_trait)
    } else {
        quote_spanned!(ret_span=> ::core::marker::Send + 'async_trait)
    };

    if static_future {
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
        sig.output = parse_quote_spanned! {ret_span=>
            -> Self::#implicit_type_name<#(#params_iter),*>
        };
    } else {
        sig.output = parse_quote_spanned! {ret_span=>
            -> ::core::pin::Pin<Box<
                dyn ::core::future::Future<Output = #ret> + #bounds
            >>
        };
    }
}

// Input:
//     async fn f<T>(&self, x: &T, (a, b): (A, B)) -> Ret {
//         self + x + a + b
//     }
//
// Output (static_future == false):
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
// Output (static_future == true):
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
fn transform_block(context: Context, sig: &mut Signature, block: &mut Block, static_future: bool) {
    if let Some(Stmt::Item(syn::Item::Verbatim(item))) = block.stmts.first() {
        if block.stmts.len() == 1 && item.to_string() == ";" {
            return;
        }
    }

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
                if let Pat::Ident(PatIdent {
                    ident, mutability, ..
                }) = &*arg.pat
                {
                    if ident == "self" {
                        self_span = Some(ident.span());
                        let prefixed = Ident::new("__self", ident.span());
                        quote!(let #mutability #prefixed = #ident;)
                    } else {
                        quote!(let #mutability #ident = #ident;)
                    }
                } else {
                    let pat = &arg.pat;
                    let ident = positional_arg(i, pat);
                    quote!(let #pat = #ident;)
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
            let _: () = { #(#stmts)* };
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

    if static_future {
        let async_block = quote_spanned!(block.brace_token.span=>
            async move { #let_ret }
        );
        block.stmts = parse_quote!(#async_block);
    } else {
        let box_pin = quote_spanned!(block.brace_token.span=>
            Box::pin(async move { #let_ret })
        );
        block.stmts = parse_quote!(#box_pin);
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
//         Self: Sync + 'async_trait,
//         'async_trait: 'life0;
fn define_implicit_associated_type(
    sig: &Signature,
    ret: &TokenStream,
    is_local: bool,
) -> TraitItemType {
    let implicit_type_name = derive_implicit_type_name(&sig.ident);
    let generated_doc = format!(
        "Automatically generated return type placeholder for [`Self::{}`]",
        sig.ident
    );
    let mut implicit_type_def: TraitItemType = if is_local {
        parse_quote!(
            #[doc = #generated_doc]
            type #implicit_type_name: ::core::future::Future<Output = #ret> + 'async_trait;
        )
    } else {
        parse_quote!(
            #[doc = #generated_doc]
            type #implicit_type_name: ::core::future::Future<Output = #ret> + ::core::marker::Send + 'async_trait;
        )
    };
    implicit_type_def.generics = sig.generics.clone();
    if let Some(receiver_lifetime) = receiver_lifetime(sig) {
        where_clause_or_default(&mut implicit_type_def.generics.where_clause)
            .predicates
            .push(parse_quote!('async_trait: #receiver_lifetime));
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
//         Self: Sync + 'async_trait
//         'async_trait: 'life0
//     = impl ::core::future::Future<Output = Ret> + 'async_trait
fn assign_implicit_associated_type(sig: &Signature, ret: &TokenStream) -> ImplItemType {
    let implicit_type_name = derive_implicit_type_name(&sig.ident);
    let generated_doc = format!(
        "Automatically generated return type for [`Self::{}`]",
        sig.ident
    );
    let mut implicit_type_assign: ImplItemType = parse_quote!(
        #[doc = #generated_doc]
        type #implicit_type_name = impl ::core::future::Future<Output = #ret> + 'async_trait;
    );
    implicit_type_assign.generics = sig.generics.clone();
    if let Some(receiver_lifetime) = receiver_lifetime(sig) {
        where_clause_or_default(&mut implicit_type_assign.generics.where_clause)
            .predicates
            .push(parse_quote!('async_trait: #receiver_lifetime));
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
    use syn::spanned::Spanned;
    format_ident!("__arg{}", i, span = pat.span())
}

fn has_bound(supertraits: &Supertraits, marker: &Ident) -> bool {
    for bound in supertraits {
        if let TypeParamBound::Trait(bound) = bound {
            if bound.path.is_ident(marker)
                || bound.path.segments.len() == 3
                    && (bound.path.segments[0].ident == "std"
                        || bound.path.segments[0].ident == "core")
                    && bound.path.segments[1].ident == "marker"
                    && bound.path.segments[2].ident == *marker
            {
                return true;
            }
        }
    }
    false
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

fn contains_static_future_attr(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if attr.path.is_ident("static_future") {
            return true;
        }
    }
    false
}

fn where_clause_or_default(clause: &mut Option<WhereClause>) -> &mut WhereClause {
    clause.get_or_insert_with(|| WhereClause {
        where_token: Default::default(),
        predicates: Punctuated::new(),
    })
}

fn derive_implicit_type_name(id: &Ident) -> Ident {
    format_ident!("RetTypeOf{}", id.to_string().to_case(Case::UpperCamel))
}

fn receiver_lifetime(sig: &Signature) -> Option<Lifetime> {
    for arg in sig.inputs.iter() {
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
