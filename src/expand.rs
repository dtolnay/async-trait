use crate::lifetime::CollectLifetimes;
use crate::parse::Item;
use crate::receiver::{has_self_in_block, has_self_in_sig, ReplaceReceiver};
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use std::mem;
use syn::punctuated::Punctuated;
use syn::visit_mut::VisitMut;
use syn::{
    parse_quote, ArgCaptured, ArgSelf, ArgSelfRef, Block, FnArg, GenericParam, Generics, Ident,
    ImplItem, Lifetime, MethodSig, Pat, PatIdent, Path, ReturnType, Token, TraitItem, Type,
    TypeParam, TypeParamBound, WhereClause,
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

pub fn expand(input: &mut Item) {
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
                            transform_block(context, sig, block, has_self);
                        }
                        let has_default = method.default.is_some();
                        transform_sig(context, sig, has_self, has_default);
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
                        transform_block(context, sig, block, has_self);
                        transform_sig(context, sig, has_self, false);
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
fn transform_sig(context: Context, sig: &mut MethodSig, has_self: bool, has_default: bool) {
    sig.decl.fn_token.span = sig.asyncness.take().unwrap().span;

    let ret = match &sig.decl.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ret) => quote!(#ret),
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
            let bound: Ident = match sig.decl.inputs.iter().next() {
                Some(FnArg::SelfRef(ArgSelfRef {
                    mutability: None, ..
                })) => parse_quote!(Sync),
                _ => parse_quote!(Send),
            };
            let assume_bound = match context {
                Context::Trait { supertraits, .. } => {
                    !has_default || has_bound(supertraits, &bound)
                }
                Context::Impl { .. } => true,
            };
            where_clause.predicates.push(if assume_bound {
                parse_quote!(Self: #lifetime)
            } else {
                parse_quote!(Self: core::marker::#bound + #lifetime)
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
        -> core::pin::Pin<Box<
            dyn core::future::Future<Output = #ret> + core::marker::Send + #lifetime
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
fn transform_block(context: Context, sig: &mut MethodSig, block: &mut Block, has_self: bool) {
    if cfg!(feature = "support_old_nightly") {
        let brace = block.brace_token;
        *block = parse_quote!({
            core::pin::Pin::from(Box::new(async move #block))
        });
        block.brace_token = brace;
        return;
    }

    let inner = Ident::new(&format!("__{}", sig.ident), sig.ident.span());
    let args = sig
        .decl
        .inputs
        .iter()
        .enumerate()
        .map(|(i, arg)| match arg {
            FnArg::SelfRef(_) | FnArg::SelfValue(_) => quote!(self),
            FnArg::Captured(ArgCaptured {
                pat: Pat::Ident(PatIdent { ident, .. }),
                ..
            }) => quote!(#ident),
            _ => positional_arg(i).into_token_stream(),
        });

    let mut standalone = sig.clone();
    standalone.ident = inner.clone();

    let outer_generics = match context {
        Context::Trait { generics, .. } => generics,
        Context::Impl { impl_generics, .. } => impl_generics,
    };
    let fn_generics = mem::replace(&mut standalone.decl.generics, outer_generics.clone());
    standalone.decl.generics.params.extend(fn_generics.params);
    if let Some(where_clause) = fn_generics.where_clause {
        standalone
            .decl
            .generics
            .make_where_clause()
            .predicates
            .extend(where_clause.predicates);
    }

    standalone
        .decl
        .generics
        .params
        .push(parse_quote!('async_trait));

    let mut types = standalone
        .decl
        .generics
        .type_params()
        .map(|param| param.ident.clone())
        .collect::<Vec<_>>();

    let mut self_bound = None::<TypeParamBound>;
    match standalone.decl.inputs.iter_mut().next() {
        Some(arg @ FnArg::SelfRef(_)) => {
            let (lifetime, mutability, self_token) = match arg {
                FnArg::SelfRef(ArgSelfRef {
                    lifetime,
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
        Some(arg @ FnArg::SelfValue(_)) => {
            let self_token = match arg {
                FnArg::SelfValue(ArgSelf { self_token, .. }) => self_token,
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
        Some(FnArg::Captured(ArgCaptured {
            pat: Pat::Ident(arg),
            ..
        })) => {
            if arg.ident == "self" {
                arg.ident = Ident::new("_self", arg.ident.span());
            }
        }
        _ => {}
    }

    if let Context::Trait { name, generics, .. } = context {
        if has_self {
            let (_, generics, _) = generics.split_for_impl();
            let mut self_param: TypeParam = parse_quote!(AsyncTrait: ?Sized + #name #generics);
            self_param.bounds.extend(self_bound);
            standalone
                .decl
                .generics
                .params
                .push(GenericParam::Type(self_param));
            types.push(Ident::new("Self", Span::call_site()));
        }
    }

    if let Some(where_clause) = &mut standalone.decl.generics.where_clause {
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
    replace.visit_method_sig_mut(&mut standalone);
    replace.visit_block_mut(block);

    let brace = block.brace_token;
    *block = parse_quote!({
        #standalone #block
        core::pin::Pin::from(Box::new(#inner::<#(#types),*>(#(#args),*)))
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
