use proc_macro2::{Group, Span, TokenStream, TokenTree};
use syn::visit_mut::{self, VisitMut};
use syn::{
    Block, ExprPath, Ident, Item, Macro, Pat, PatIdent, PatPath, Receiver, Signature, Token,
    TypePath,
};

pub fn has_self_in_sig(sig: &mut Signature) -> bool {
    let mut visitor = HasSelf(false);
    visitor.visit_signature_mut(sig);
    visitor.0
}

pub fn has_self_in_block(block: &mut Block) -> bool {
    let mut visitor = HasSelf(false);
    visitor.visit_block_mut(block);
    visitor.0
}

fn has_self_in_token_stream(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|tt| match tt {
        TokenTree::Ident(ident) => ident == "Self",
        TokenTree::Group(group) => has_self_in_token_stream(group.stream()),
        _ => false,
    })
}

pub fn mut_pat(pat: &mut Pat) -> Option<Token![mut]> {
    let mut visitor = HasMutPat(None);
    visitor.visit_pat_mut(pat);
    visitor.0
}

fn contains_fn(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|tt| match tt {
        TokenTree::Ident(ident) => ident == "fn",
        TokenTree::Group(group) => contains_fn(group.stream()),
        _ => false,
    })
}

struct HasMutPat(Option<Token![mut]>);

impl VisitMut for HasMutPat {
    fn visit_pat_ident_mut(&mut self, i: &mut PatIdent) {
        if let Some(m) = i.mutability {
            self.0 = Some(m);
        } else {
            visit_mut::visit_pat_ident_mut(self, i);
        }
    }
}

struct HasSelf(bool);

impl VisitMut for HasSelf {
    fn visit_expr_path_mut(&mut self, expr: &mut ExprPath) {
        self.0 |= expr.path.segments[0].ident == "Self";
        visit_mut::visit_expr_path_mut(self, expr);
    }

    fn visit_pat_path_mut(&mut self, pat: &mut PatPath) {
        self.0 |= pat.path.segments[0].ident == "Self";
        visit_mut::visit_pat_path_mut(self, pat);
    }

    fn visit_type_path_mut(&mut self, ty: &mut TypePath) {
        self.0 |= ty.path.segments[0].ident == "Self";
        visit_mut::visit_type_path_mut(self, ty);
    }

    fn visit_receiver_mut(&mut self, _arg: &mut Receiver) {
        self.0 = true;
    }

    fn visit_item_mut(&mut self, _: &mut Item) {
        // Do not recurse into nested items.
    }

    fn visit_macro_mut(&mut self, mac: &mut Macro) {
        if !contains_fn(mac.tokens.clone()) {
            self.0 |= has_self_in_token_stream(mac.tokens.clone());
        }
    }
}

pub struct ReplaceSelf<'a>(pub &'a str, pub Span);

impl ReplaceSelf<'_> {
    fn visit_token_stream(&mut self, tt: TokenStream) -> TokenStream {
        tt.into_iter()
            .map(|tt| match tt {
                TokenTree::Ident(mut ident) => {
                    self.visit_ident_mut(&mut ident);
                    TokenTree::Ident(ident)
                }
                TokenTree::Group(group) => {
                    let tt = self.visit_token_stream(group.stream());
                    let mut new = Group::new(group.delimiter(), tt);
                    new.set_span(group.span());
                    TokenTree::Group(new)
                }
                tt => tt,
            })
            .collect()
    }
}

impl VisitMut for ReplaceSelf<'_> {
    fn visit_ident_mut(&mut self, i: &mut Ident) {
        if i == "self" {
            *i = quote::format_ident!("{}{}", self.0, i);
            #[cfg(self_span_hack)]
            i.set_span(self.1);
        }

        visit_mut::visit_ident_mut(self, i);
    }

    fn visit_item_mut(&mut self, i: &mut Item) {
        // Visit `macro_rules!` because locally defined macros can refer to
        // `self`. Otherwise, do not recurse into nested items.
        if let Item::Macro(i) = i {
            if i.mac.path.is_ident("macro_rules") {
                self.visit_macro_mut(&mut i.mac)
            }
        }
    }

    fn visit_macro_mut(&mut self, mac: &mut Macro) {
        mac.tokens = self.visit_token_stream(mac.tokens.clone());
        visit_mut::visit_macro_mut(self, mac);
    }
}
