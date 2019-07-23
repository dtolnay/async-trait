use std::mem;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, ExprPath, Item, Path, QSelf, Type, TypePath};

pub struct ReplaceReceiver {
    pub with: Type,
    pub as_trait: Option<Path>,
}

impl ReplaceReceiver {
    pub fn with(ty: Type) -> Self {
        ReplaceReceiver {
            with: ty,
            as_trait: None,
        }
    }

    pub fn with_as_trait(ty: Type, as_trait: Path) -> Self {
        ReplaceReceiver {
            with: ty,
            as_trait: Some(as_trait),
        }
    }

    fn self_to_qself_type(&self, qself: &mut Option<QSelf>, path: &mut Path) {
        self.self_to_qself(qself, path, true);
    }

    fn self_to_qself_expr(&self, qself: &mut Option<QSelf>, path: &mut Path) {
        self.self_to_qself(qself, path, false);
    }

    fn self_to_qself(&self, qself: &mut Option<QSelf>, path: &mut Path, include_as_trait: bool) {
        if path.leading_colon.is_some() {
            return;
        }

        let first = &path.segments[0];
        if first.ident != "Self" || !first.arguments.is_empty() {
            return;
        }

        *qself = Some(QSelf {
            lt_token: Default::default(),
            ty: Box::new(self.with.clone()),
            position: 0,
            as_token: None,
            gt_token: Default::default(),
        });

        if include_as_trait && self.as_trait.is_some() {
            let as_trait = self.as_trait.as_ref().unwrap().clone();
            path.leading_colon = as_trait.leading_colon;
            qself.as_mut().unwrap().position = as_trait.segments.len();

            let segments = mem::replace(&mut path.segments, as_trait.segments);
            path.segments.push_punct(Default::default());
            path.segments.extend(segments.into_pairs().skip(1));
        } else {
            path.leading_colon = Some(**path.segments.first().unwrap().punct().unwrap());

            let segments = mem::replace(&mut path.segments, Punctuated::new());
            path.segments = segments.into_pairs().skip(1).collect();
        }
    }
}

impl VisitMut for ReplaceReceiver {
    // `Self` -> `Receiver`
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(node) = ty {
            if node.qself.is_none() && node.path.is_ident("Self") {
                *ty = self.with.clone();
            } else {
                self.visit_type_path_mut(node);
            }
        } else {
            visit_mut::visit_type_mut(self, ty);
        }
    }

    // `Self::Assoc` -> `<Receiver>::Assoc`
    fn visit_type_path_mut(&mut self, ty: &mut TypePath) {
        if ty.qself.is_none() {
            self.self_to_qself_type(&mut ty.qself, &mut ty.path);
        }
        visit_mut::visit_type_path_mut(self, ty);
    }

    // `Self::method` -> `<Receiver>::method`
    fn visit_expr_path_mut(&mut self, expr: &mut ExprPath) {
        if expr.qself.is_none() {
            if expr.path.is_ident("self") {
                expr.path.segments[0].ident = parse_quote!(_self);
            }
            self.self_to_qself_expr(&mut expr.qself, &mut expr.path);
        }
        visit_mut::visit_expr_path_mut(self, expr);
    }

    fn visit_item_mut(&mut self, _: &mut Item) {
        // Do not recurse into nested items.
    }
}
