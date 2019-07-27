use proc_macro2::Span;
use syn::visit_mut::{self, VisitMut};
use syn::{ArgSelfRef, Block, GenericArgument, Item, Lifetime, MethodSig, TypeReference};

pub fn has_async_lifetime(sig: &mut MethodSig, block: &mut Block) -> bool {
    let mut visitor = HasAsyncLifetime(false);
    visitor.visit_method_sig_mut(sig);
    visitor.visit_block_mut(block);
    visitor.0
}

struct HasAsyncLifetime(bool);

impl VisitMut for HasAsyncLifetime {
    fn visit_lifetime_mut(&mut self, life: &mut Lifetime) {
        self.0 |= life.to_string() == "'async_trait";
    }

    fn visit_item_mut(&mut self, _: &mut Item) {
        // Do not recurse into nested items.
    }
}

pub struct CollectLifetimes {
    pub lifetimes: Vec<Lifetime>,
}

impl CollectLifetimes {
    pub fn new() -> Self {
        CollectLifetimes {
            lifetimes: Vec::new(),
        }
    }

    fn visit_opt_lifetime(&mut self, lifetime: &mut Option<Lifetime>) {
        match lifetime {
            None => *lifetime = Some(self.next_lifetime()),
            Some(lifetime) => self.visit_lifetime(lifetime),
        }
    }

    fn visit_lifetime(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == "_" {
            *lifetime = self.next_lifetime();
        }
    }

    fn next_lifetime(&mut self) -> Lifetime {
        let name = format!("'life{}", self.lifetimes.len());
        let life = Lifetime::new(&name, Span::call_site());
        self.lifetimes.push(life.clone());
        life
    }
}

impl VisitMut for CollectLifetimes {
    fn visit_arg_self_ref_mut(&mut self, arg: &mut ArgSelfRef) {
        self.visit_opt_lifetime(&mut arg.lifetime);
    }

    fn visit_type_reference_mut(&mut self, ty: &mut TypeReference) {
        self.visit_opt_lifetime(&mut ty.lifetime);
        visit_mut::visit_type_reference_mut(self, ty);
    }

    fn visit_generic_argument_mut(&mut self, gen: &mut GenericArgument) {
        if let GenericArgument::Lifetime(lifetime) = gen {
            self.visit_lifetime(lifetime);
        }
        visit_mut::visit_generic_argument_mut(self, gen);
    }
}
