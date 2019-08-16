use crate::lifetime::{has_async_lifetime, CollectLifetimes};
use crate::parse::Item;
use crate::receiver::{has_self_in_block, has_self_in_sig, ReplaceReceiver};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use std::mem;
use syn::punctuated::Punctuated;
use syn::visit_mut::VisitMut;
use syn::{
    parse_quote, Block, FnArg, GenericParam, Generics, Ident, ImplItem, Lifetime, Pat, PatIdent,
    Path, Receiver, ReturnType, Signature, Token, TraitItem, Type, TypeParam, TypeParamBound,
    WhereClause,
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
        name: &'a Ident,
        generics: &'a Generics,
        supertraits: &'a Supertraits,
    },
    Impl {
        impl_generics: &'a Generics,
        receiver: &'a Type,
        as_trait: &'a Path,
    },
}

type Supertraits = Punctuated<TypeParamBound, Token![+]>;

pub fn expand(input: &mut Item, is_local: bool) {
    match input {
        Item::Trait(input) => {
            let context = Context::Trait {
                name: &input.ident,
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
                            transform_block(context, sig, block, has_self, is_local);
                        }
                        let has_default = method.default.is_some();
                        transform_sig(context, sig, has_self, has_default, is_local);
                    }
                }
            }
        }
        Item::Impl(input) => {
            let context = Context::Impl {
                impl_generics: &input.generics,
                receiver: &input.self_ty,
                as_trait: &input.trait_.as_ref().unwrap().1,
            };
            for inner in &mut input.items {
                if let ImplItem::Method(method) = inner {
                    let sig = &mut method.sig;
                    if sig.asyncness.is_some() {
                        let block = &mut method.block;
                        let has_self = has_self_in_sig(sig) || has_self_in_block(block);
                        transform_block(context, sig, block, has_self, is_local);
                        transform_sig(context, sig, has_self, false, is_local);
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
fn transform_sig(
    context: Context,
    sig: &mut Signature,
    has_self: bool,
    has_default: bool,
    is_local: bool,
) {
    sig.fn_token.span = sig.asyncness.take().unwrap().span;

    let ret = match &sig.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret) => quote!(#ret),
    };

    let mut elided = CollectLifetimes::new();
    for arg in sig.inputs.iter_mut() {
        match arg {
            FnArg::Receiver(arg) => elided.visit_receiver_mut(arg),
            FnArg::Typed(arg) => elided.visit_type_mut(&mut arg.ty),
        }
    }

    let lifetime: Lifetime;
    if !sig.generics.params.is_empty() || !elided.lifetimes.is_empty() || has_self {
        lifetime = parse_quote!('async_trait);
        let where_clause = sig
            .generics
            .where_clause
            .get_or_insert_with(|| WhereClause {
                where_token: Default::default(),
                predicates: Punctuated::new(),
            });
        for param in &sig.generics.params {
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
            sig.generics.params.push(parse_quote!(#elided));
            where_clause
                .predicates
                .push(parse_quote!(#elided: #lifetime));
        }
        sig.generics.params.push(parse_quote!(#lifetime));
        if has_self {
            let bound: Ident = match sig.inputs.iter().next() {
                Some(FnArg::Receiver(Receiver {
                    reference: Some(_),
                    mutability: None,
                    ..
                })) => parse_quote!(Sync),
                _ => parse_quote!(Send),
            };
            let assume_bound = match context {
                Context::Trait { supertraits, .. } => {
                    !has_default || has_bound(supertraits, &bound)
                }
                Context::Impl { .. } => true,
            };
            where_clause.predicates.push(if assume_bound || is_local {
                parse_quote!(Self: #lifetime)
            } else {
                parse_quote!(Self: core::marker::#bound + #lifetime)
            });
        }
    } else {
        lifetime = parse_quote!('static);
    };

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
                    let positional = positional_arg(i);
                    *arg.pat = parse_quote!(#positional);
                }
            }
        }
    }

    let bounds = if is_local {
        quote!(#lifetime)
    } else {
        quote!(core::marker::Send + #lifetime)
    };

    sig.output = parse_quote! {
        -> core::pin::Pin<Box<
            dyn core::future::Future<Output = #ret> + #bounds
        >>
    };
}

// Input:
//     async fn f<T>(&self, x: &T) -> Ret {
//         self + x
//     }
//
// Output:
//     async fn f<T, AsyncTrait>(_self: &AsyncTrait, x: &T) -> Ret {
//         _self + x
//     }
//     Pin::from(Box::new(async_trait_method::<T, Self>(self, x)))
fn transform_block(
    context: Context,
    sig: &mut Signature,
    block: &mut Block,
    has_self: bool,
    is_local: bool,
) {
    if cfg!(feature = "support_old_nightly") {
        let brace = block.brace_token;
        *block = parse_quote!({
            core::pin::Pin::from(Box::new(async move #block))
        });
        block.brace_token = brace;
        return;
    }

    let inner = format_ident!("__{}", sig.ident);
    let args = sig.inputs.iter().enumerate().map(|(i, arg)| match arg {
        FnArg::Receiver(_) => quote!(self),
        FnArg::Typed(arg) => {
            if let Pat::Ident(PatIdent { ident, .. }) = &*arg.pat {
                quote!(#ident)
            } else {
                positional_arg(i).into_token_stream()
            }
        }
    });

    let mut standalone = sig.clone();
    standalone.ident = inner.clone();

    let outer_generics = match context {
        Context::Trait { generics, .. } => generics,
        Context::Impl { impl_generics, .. } => impl_generics,
    };
    let fn_generics = mem::replace(&mut standalone.generics, outer_generics.clone());
    standalone.generics.params.extend(fn_generics.params);
    if let Some(where_clause) = fn_generics.where_clause {
        standalone
            .generics
            .make_where_clause()
            .predicates
            .extend(where_clause.predicates);
    }

    if has_async_lifetime(&mut standalone, block) {
        standalone.generics.params.push(parse_quote!('async_trait));
    }

    let mut types = standalone
        .generics
        .type_params()
        .map(|param| param.ident.clone())
        .collect::<Vec<_>>();

    let mut self_bound = None::<TypeParamBound>;
    match standalone.inputs.iter_mut().next() {
        Some(
            arg @ FnArg::Receiver(Receiver {
                reference: Some(_), ..
            }),
        ) => {
            let (lifetime, mutability, self_token) = match arg {
                FnArg::Receiver(Receiver {
                    reference: Some((_, lifetime)),
                    mutability,
                    self_token,
                    ..
                }) => (lifetime, mutability, self_token),
                _ => unreachable!(),
            };
            let under_self = Ident::new("_self", self_token.span);
            match context {
                Context::Trait { .. } => {
                    self_bound = Some(match mutability {
                        Some(_) => parse_quote!(core::marker::Send),
                        None => parse_quote!(core::marker::Sync),
                    });
                    *arg = parse_quote! {
                        #under_self: &#lifetime #mutability AsyncTrait
                    };
                }
                Context::Impl { receiver, .. } => {
                    *arg = parse_quote! {
                        #under_self: &#lifetime #mutability #receiver
                    };
                }
            }
        }
        Some(arg @ FnArg::Receiver(_)) => {
            let self_token = match arg {
                FnArg::Receiver(Receiver { self_token, .. }) => self_token,
                _ => unreachable!(),
            };
            let under_self = Ident::new("_self", self_token.span);
            match context {
                Context::Trait { .. } => {
                    self_bound = Some(parse_quote!(core::marker::Send));
                    *arg = parse_quote! {
                        #under_self: AsyncTrait
                    };
                }
                Context::Impl { receiver, .. } => {
                    *arg = parse_quote! {
                        #under_self: #receiver
                    };
                }
            }
        }
        Some(FnArg::Typed(arg)) => {
            if let Pat::Ident(arg) = &mut *arg.pat {
                if arg.ident == "self" {
                    arg.ident = Ident::new("_self", arg.ident.span());
                }
            }
        }
        _ => {}
    }

    if let Context::Trait { name, generics, .. } = context {
        if has_self {
            let (_, generics, _) = generics.split_for_impl();
            let mut self_param: TypeParam = parse_quote!(AsyncTrait: ?Sized + #name #generics);
            if !is_local {
                self_param.bounds.extend(self_bound);
            }
            standalone
                .generics
                .params
                .push(GenericParam::Type(self_param));
            types.push(Ident::new("Self", Span::call_site()));
        }
    }

    if let Some(where_clause) = &mut standalone.generics.where_clause {
        // Work around an input bound like `where Self::Output: Send` expanding
        // to `where <AsyncTrait>::Output: Send` which is illegal syntax because
        // `where<T>` is reserved for future use... :(
        where_clause.predicates.insert(0, parse_quote!((): Sized));
    }

    let mut replace = match context {
        Context::Trait { .. } => ReplaceReceiver::with(parse_quote!(AsyncTrait)),
        Context::Impl {
            receiver, as_trait, ..
        } => ReplaceReceiver::with_as_trait(receiver.clone(), as_trait.clone()),
    };
    replace.visit_signature_mut(&mut standalone);
    replace.visit_block_mut(block);

    let brace = block.brace_token;
    *block = parse_quote!({
        #[allow(clippy::used_underscore_binding)]
        #standalone #block
        core::pin::Pin::from(Box::new(#inner::<#(#types),*>(#(#args),*)))
    });
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
