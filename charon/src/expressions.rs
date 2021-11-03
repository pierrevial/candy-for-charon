//! Implements expressions: paths, operands, rvalues, lvalues

use crate::common::*;
use crate::formatter::Formatter;
use crate::types::*;
use crate::values;
use crate::values::*;
use im::Vector;
use macros::{EnumAsGetters, EnumIsA, VariantName};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Place {
    pub var_id: VarId::Id,
    pub projection: Projection,
}

pub type Projection = Vector<ProjectionElem>;

#[derive(Debug, PartialEq, Eq, Clone, VariantName, Serialize)]
pub enum ProjectionElem {
    /// Dereference a shared/mutable reference.
    Deref,
    /// Dereference a boxed value.
    /// Note that this doesn't exist in MIR where `Deref` is used both for the
    /// mutable and shared references *and* the boxed values. As semantically we
    /// don't handle those two cases the same way at all, we disambiguate them
    /// during the translation.
    /// In rust, this comes from the `*` operator applied on boxes.
    DerefBox,
    /// Projection from ADTs (variants, structures).
    /// We allow projections to be used as left-values and right-values.
    /// We should never have projections to fields of symbolic variants (they
    /// should have been expanded before through a match).
    /// Note that in MIR, field projections don't contain any type information
    /// (adt identifier, variant id, etc.). This information can be valuable
    /// (for pretty printing for instance). We retrieve it through
    /// type-checking.
    Field(FieldProjKind, FieldId::Id),
    /// Downcast of an enumeration to a specific variant.
    /// For example, the left value in:
    /// `((_0 as Right).0: T2) = move _1;`
    /// Note that the downcast is always performed *before* the field projection.
    /// This means that we can use it to correctly expand `Bottom` values.
    /// Note that MIR uses downcasts because the variant fields are initialized
    /// one by one. When initializing a variant value (which is thus initially
    /// `Bottom`), we use the first downcast to freeze the enumeration to the proper
    /// variant, by replacing the value `Bottom` with `Variant_i Bottom ... Bottom`.
    /// Note that we can't use the `SetDiscriminant` statement, because it always
    /// happens *after* the fields have been initialized... Upon executing a
    /// `SetDiscriminant` statement, we just check that the variant is the proper
    /// one (for sanity).
    Downcast(VariantId::Id),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, EnumAsGetters, Serialize)]
pub enum FieldProjKind {
    Adt(TypeDefId::Id, Option<VariantId::Id>),
    /// If we project from a tuple, the projection kind gives the arity of the
    Tuple(usize),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, EnumAsGetters, Serialize)]
pub enum BorrowKind {
    Shared,
    Mut,
    TwoPhaseMut,
}

/// Unary operation
#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, VariantName, Serialize)]
pub enum UnOp {
    Not,
    /// This can overflow. In practice, rust introduces an assert before
    /// (in debug mode) to check that it is not equal to the minimum integer
    /// value (for the proper type). In our semantics, we leave the value as
    /// it is in case of overflow.
    Neg,
}

/// Binary operations.
#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, VariantName, Serialize)]
pub enum BinOp {
    BitXor,
    BitAnd,
    BitOr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    /// Can fail if the divisor is 0.
    Div,
    /// Can fail if the divisor is 0.
    Rem,
    /// Can overflow
    Add,
    /// Can overflow
    Sub,
    /// Can overflow
    Mul,
    /// Can fail if the shift is too big
    Shl,
    /// Can fail if the shift is too big
    Shr,
    // No Offset binary operation: this is an operation on raw pointers
}

impl Serialize for Place {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("Place", 2)?;
        s.serialize_field("var_id", &self.var_id)?;
        let projection = VectorSerializer::new(&self.projection);
        s.serialize_field("projection", &projection)?;
        s.end()
    }
}

impl std::fmt::Display for BorrowKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            BorrowKind::Shared => write!(f, "Shared"),
            BorrowKind::Mut => write!(f, "Mut"),
            BorrowKind::TwoPhaseMut => write!(f, "TwoPhaseMut"),
        }
    }
}

impl std::string::ToString for UnOp {
    fn to_string(&self) -> String {
        match self {
            UnOp::Not => "~".to_owned(),
            UnOp::Neg => "-".to_owned(),
        }
    }
}

impl std::string::ToString for BinOp {
    fn to_string(&self) -> String {
        match self {
            BinOp::BitXor => "^".to_owned(),
            BinOp::BitAnd => "&".to_owned(),
            BinOp::BitOr => "|".to_owned(),
            BinOp::Eq => "==".to_owned(),
            BinOp::Lt => "<".to_owned(),
            BinOp::Le => "<=".to_owned(),
            BinOp::Ne => "!=".to_owned(),
            BinOp::Ge => ">=".to_owned(),
            BinOp::Gt => ">".to_owned(),
            BinOp::Div => "/".to_owned(),
            BinOp::Rem => "%".to_owned(),
            BinOp::Add => "+".to_owned(),
            BinOp::Sub => "-".to_owned(),
            BinOp::Mul => "*".to_owned(),
            BinOp::Shl => "<<".to_owned(),
            BinOp::Shr => ">>".to_owned(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, EnumIsA, EnumAsGetters, VariantName, Serialize)]
pub enum Operand {
    Copy(Place),
    Move(Place),
    /// Constant value.
    Constant(ETy, OperandConstantValue),
}

/// Constant value for an operand.
///
/// It is a bit annoying, but Rust treats some ADT and tuple instances as
/// constants.
/// For instance, an enumeration with one variant and no fields is a constant.
/// A structure with no field is a constant.
///
/// For our translation, we use the following enumeration to encode those
/// special cases in assignments. They are converted to "normal" values
/// when evaluating the assignment (which is why we don't put them in the
/// [`ConstantValue`](crate::ConstantValue) enumeration.
#[derive(Debug, PartialEq, Eq, Clone, VariantName, Serialize)]
pub enum OperandConstantValue {
    ConstantValue(ConstantValue),
    /// Enumeration with one variant with no fields, or structure with
    /// no fields.
    Adt(TypeDefId::Id),
    /// In MIR, unit is actually encoded as a 0-tuple
    Unit,
}

#[derive(Debug, Clone, Serialize)]
pub enum Rvalue {
    Use(Operand),
    Ref(Place, BorrowKind),
    /// Unary operation (not, neg)
    UnaryOp(UnOp, Operand),
    /// Binary operations (note that we merge "checked" and "unchecked" binops)
    BinaryOp(BinOp, Operand, Operand),
    /// Discriminant (for enumerations).
    /// Note that discriminant values have type isize
    Discriminant(Place),
    /// Creates an aggregate value, like a tuple, a struct or an enum:
    /// ```
    /// l = List::Cons { value:x, tail:tl };
    /// ```
    /// Note that in some MIR passes (like optimized MIR), aggregate values are
    /// decomposed, like below:
    /// ```
    /// (l as List::Cons).value = x;
    /// (l as List::Cons).tail = tl;
    /// ```
    /// Because we may want to plug our translation mechanism at various
    /// places, we need to take both into accounts in the translation and in
    /// our semantics. Aggregate value initialization is easy, you might want
    /// to have a look at expansion of `Bottom` values for explanations about the
    /// other case.
    Aggregate(AggregateKind, Vec<Operand>),
}

#[derive(Debug, Clone, Serialize)]
pub enum AggregateKind {
    Tuple,
    Adt(TypeDefId::Id, Option<VariantId::Id>),
}

impl Place {
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<VarId::Id> + Formatter<(TypeDefId::Id, Option<VariantId::Id>, FieldId::Id)>,
    {
        let mut out = ctx.format_object(self.var_id);

        for p in &self.projection {
            match p {
                ProjectionElem::Deref => {
                    out = format!("*({})", out);
                }
                ProjectionElem::DerefBox => {
                    out = format!("deref_box ({})", out);
                }
                ProjectionElem::Field(proj_kind, field_id) => match proj_kind {
                    FieldProjKind::Adt(adt_id, opt_variant_id) => {
                        let field_name = ctx.format_object((*adt_id, *opt_variant_id, *field_id));
                        out = format!("({}).{}", out, field_name);
                    }
                    FieldProjKind::Tuple(_) => {
                        out = format!("({}).{}", out, field_id);
                    }
                },
                ProjectionElem::Downcast(variant_id) => {
                    out = format!("({} as variant @{})", out, variant_id);
                }
            }
        }

        out
    }

    /// Perform a type substitution - actually simply clone the object
    pub fn substitute(&self, _subst: &ETypeSubst) -> Self {
        self.clone()
    }
}

impl std::string::ToString for Place {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&values::DummyFormatter {})
    }
}

impl OperandConstantValue {
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<TypeDefId::Id>,
    {
        match self {
            OperandConstantValue::ConstantValue(c) => c.to_string(),
            OperandConstantValue::Adt(def_id) => {
                format!("ConstAdt {}", ctx.format_object(*def_id)).to_owned()
            }
            OperandConstantValue::Unit => "()".to_owned(),
        }
    }
}

impl std::string::ToString for OperandConstantValue {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&values::DummyFormatter {})
    }
}

impl Operand {
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<VarId::Id>
            + Formatter<TypeDefId::Id>
            + Formatter<(TypeDefId::Id, Option<VariantId::Id>, FieldId::Id)>,
    {
        match self {
            Operand::Copy(p) => format!("copy ({})", p.fmt_with_ctx(ctx)).to_owned(),
            Operand::Move(p) => format!("move ({})", p.fmt_with_ctx(ctx)).to_owned(),
            Operand::Constant(_, c) => format!("const ({})", c.fmt_with_ctx(ctx)).to_owned(),
        }
    }

    /// Perform a type substitution - actually simply clone the object
    pub fn substitute(&self, _subst: &ETypeSubst) -> Self {
        self.clone()
    }
}

impl std::string::ToString for Operand {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&values::DummyFormatter {})
    }
}

impl Rvalue {
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<VarId::Id>
            + Formatter<TypeDefId::Id>
            + Formatter<(TypeDefId::Id, VariantId::Id)>
            + Formatter<(TypeDefId::Id, Option<VariantId::Id>, FieldId::Id)>,
    {
        match self {
            Rvalue::Use(x) => x.fmt_with_ctx(ctx),
            Rvalue::Ref(place, borrow_kind) => match borrow_kind {
                BorrowKind::Shared => format!("&{}", place.fmt_with_ctx(ctx)).to_owned(),
                BorrowKind::Mut => format!("&mut {}", place.fmt_with_ctx(ctx)).to_owned(),
                BorrowKind::TwoPhaseMut => {
                    format!("&two-phase-mut {}", place.fmt_with_ctx(ctx)).to_owned()
                }
            },
            Rvalue::UnaryOp(unop, x) => {
                format!("{}({})", unop.to_string(), x.fmt_with_ctx(ctx)).to_owned()
            }
            Rvalue::BinaryOp(binop, x, y) => format!(
                "{} {} {}",
                x.fmt_with_ctx(ctx),
                binop.to_string(),
                y.fmt_with_ctx(ctx)
            )
            .to_owned(),
            Rvalue::Discriminant(p) => {
                format!("@discriminant({})", p.fmt_with_ctx(ctx),).to_owned()
            }
            Rvalue::Aggregate(kind, ops) => {
                let ops_s: Vec<String> = ops.iter().map(|op| op.fmt_with_ctx(ctx)).collect();
                match kind {
                    AggregateKind::Tuple => format!("({})", ops_s.join(", ")).to_owned(),
                    AggregateKind::Adt(def_id, variant_id) => {
                        // Format every field
                        let mut fields = vec![];
                        for i in 0..ops.len() {
                            let field_id = FieldId::Id::new(i);
                            let field_name = ctx.format_object((*def_id, *variant_id, field_id));
                            let op = &ops[i];
                            fields.push(
                                format!("{}: {}", field_name, op.fmt_with_ctx(ctx)).to_owned(),
                            );
                        }

                        let variant = match variant_id {
                            None => ctx.format_object(*def_id),
                            Some(variant_id) => ctx.format_object((*def_id, *variant_id)),
                        };
                        format!("{} {{ {} }}", variant, fields.join(", "))
                    }
                }
            }
        }
    }

    /// Perform a type substitution - actually simply clone the object
    pub fn substitute(&self, _subst: &ETypeSubst) -> Self {
        self.clone()
    }
}

impl std::string::ToString for Rvalue {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&values::DummyFormatter {})
    }
}
