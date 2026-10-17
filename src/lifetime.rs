use proc_macro2::{Span, TokenStream};
use std::mem;
use syn::visit_mut::{self, VisitMut};
use syn::{
    parse_quote_spanned, token, Expr, GenericArgument, Lifetime, ReceiverKind, ReturnType, Token,
    Type, TypeFnPtr, TypeImplTrait, TypeParen, TypePtr, TypeReference,
};

pub struct CollectLifetimes {
    pub elided: Vec<Lifetime>,
    pub explicit: Vec<Lifetime>,
    reference_depth: usize,
    impl_trait_depth: usize,
}

impl CollectLifetimes {
    pub fn new() -> Self {
        CollectLifetimes {
            elided: Vec::new(),
            explicit: Vec::new(),
            reference_depth: 0,
            impl_trait_depth: 0,
        }
    }

    fn visit_opt_lifetime(&mut self, reference: &Token![&], lifetime: &mut Option<Lifetime>) {
        match lifetime {
            None => *lifetime = Some(self.next_lifetime(reference.span)),
            Some(lifetime) => self.visit_lifetime(lifetime),
        }
    }

    fn visit_lifetime(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == "_" {
            *lifetime = self.next_lifetime(lifetime.span());
        } else {
            self.explicit.push(lifetime.clone());
        }
    }

    fn next_lifetime(&mut self, span: Span) -> Lifetime {
        let name = format!("'life{}", self.elided.len());
        let life = Lifetime::new(&name, span);
        self.elided.push(life.clone());
        life
    }
}

impl VisitMut for CollectLifetimes {
    fn visit_receiver_kind_mut(&mut self, receiver: &mut ReceiverKind) {
        match receiver {
            ReceiverKind::Value => {}
            ReceiverKind::Reference(reference, lifetime, _mutability) => {
                self.visit_opt_lifetime(reference, lifetime);
            }
            ReceiverKind::Typed(_colon, ty) => {
                visit_mut::visit_type_mut(self, ty);
            }
            _ => {}
        }
    }

    fn visit_type_reference_mut(&mut self, ty: &mut TypeReference) {
        self.visit_opt_lifetime(&ty.and_token, &mut ty.lifetime);
        self.reference_depth += 1;
        self.visit_type_mut(&mut ty.elem);
        self.reference_depth -= 1;
    }

    fn visit_generic_argument_mut(&mut self, gen: &mut GenericArgument) {
        if let GenericArgument::Lifetime(lifetime) = gen {
            // A borrowed type's elided path lifetimes are independent of the
            // reference lifetime. Keep placeholders elided, just like omitted
            // path lifetimes; the borrow supplies their outlives requirement.
            if lifetime.ident != "_" || self.reference_depth == 0 || self.impl_trait_depth > 0 {
                self.visit_lifetime(lifetime);
            }
        }
        visit_mut::visit_generic_argument_mut(self, gen);
    }

    fn visit_type_impl_trait_mut(&mut self, ty: &mut TypeImplTrait) {
        self.impl_trait_depth += 1;
        visit_mut::visit_type_impl_trait_mut(self, ty);
        self.impl_trait_depth -= 1;
    }
}

pub struct AddLifetimeToImplTrait;

impl VisitMut for AddLifetimeToImplTrait {
    fn visit_type_impl_trait_mut(&mut self, ty: &mut TypeImplTrait) {
        let span = ty.impl_token.span;
        let lifetime = parse_quote_spanned!(span=> 'async_trait);
        ty.bounds.insert(0, lifetime);
        if let Some(punct) = ty.bounds.pairs_mut().next().unwrap().punct_mut() {
            punct.span = span;
        }
        visit_mut::visit_type_impl_trait_mut(self, ty);
    }

    fn visit_type_reference_mut(&mut self, ty: &mut TypeReference) {
        parenthesize_impl_trait(&mut ty.elem, ty.and_token.span);
        visit_mut::visit_type_reference_mut(self, ty);
    }

    fn visit_type_ptr_mut(&mut self, ty: &mut TypePtr) {
        parenthesize_impl_trait(&mut ty.elem, ty.star_token.span);
        visit_mut::visit_type_ptr_mut(self, ty);
    }

    fn visit_type_fn_ptr_mut(&mut self, ty: &mut TypeFnPtr) {
        if let ReturnType::Type(arrow, return_type) = &mut ty.output {
            parenthesize_impl_trait(return_type, arrow.spans[0]);
        }
        visit_mut::visit_type_fn_ptr_mut(self, ty);
    }

    fn visit_expr_mut(&mut self, _e: &mut Expr) {
        // Do not recurse into impl Traits inside of an array length expression.
        //
        //    fn outer(arg: [u8; { fn inner(_: impl Trait) {}; 0 }]);
    }
}

fn parenthesize_impl_trait(elem: &mut Type, paren_span: Span) {
    if let Type::ImplTrait(_) = *elem {
        let placeholder = Type::Verbatim(TokenStream::new());
        *elem = Type::Paren(TypeParen {
            attrs: Vec::new(),
            paren_token: token::Paren(paren_span),
            elem: Box::new(mem::replace(elem, placeholder)),
        });
    }
}
