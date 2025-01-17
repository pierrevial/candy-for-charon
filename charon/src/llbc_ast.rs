//! LLBC
//!
//! MIR code where we have rebuilt the control-flow (`if ... then ... else ...`,
//! `while ...`, ...).
//!
//! Also note that we completely break the definitions Statement and Terminator
//! from MIR to use Statement only.

#![allow(dead_code)]
use crate::expressions::*;
pub use crate::llbc_ast_utils::*;
use crate::meta::Meta;
use crate::types::*;
use crate::ullbc_ast::*;
pub use crate::ullbc_ast::{CtxNames, FunDeclId, GlobalDeclId, Var};
use crate::values::*;
use macros::{EnumAsGetters, EnumIsA, EnumToGetters, VariantIndexArity, VariantName};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Assert {
    pub cond: Operand,
    pub expected: bool,
}

/// TODO: factor out with [Rvalue]
#[derive(Debug, Clone, Serialize)]
pub struct Call {
    pub func: FunId,
    /// Technically this is useless, but we still keep it because we might
    /// want to introduce some information (and the way we encode from MIR
    /// is as simple as possible - and in MIR we also have a vector of erased
    /// regions).
    pub region_args: Vec<ErasedRegion>,
    pub type_args: Vec<ETy>,
    pub args: Vec<Operand>,
    pub dest: Place,
}

/// A raw statement: a statement without meta data.
#[derive(Debug, Clone, EnumIsA, EnumToGetters, EnumAsGetters, Serialize)]
pub enum RawStatement<R> where
R: Clone + std::cmp::Eq, {
    Assign(Place, Rvalue<R>),
    FakeRead(Place),
    SetDiscriminant(Place, VariantId::Id),
    Drop(Place),
    Assert(Assert),
    Call(Call),
    /// Panic also handles "unreachable"
    Panic,
    Return,
    /// Break to outer loops.
    /// The `usize` gives the index of the outer loop to break to:
    /// * 0: break to first outer loop (the current loop)
    /// * 1: break to second outer loop
    /// * ...
    Break(usize),
    /// Continue to outer loops.
    /// The `usize` gives the index of the outer loop to continue to:
    /// * 0: continue to first outer loop (the current loop)
    /// * 1: continue to second outer loop
    /// * ...
    Continue(usize),
    /// No-op.
    Nop,
    /// The left statement must NOT be a sequence.
    /// For instance, `(s0; s1); s2` is forbidden and should be rewritten
    /// to the semantically equivalent statement `s0; (s1; s2)`
    /// To ensure that, use [crate::llbc_ast_utils::new_sequence] to build sequences.
    Sequence(Box<Statement<R>>, Box<Statement<R>>),
    Switch(Switch<R>),
    Loop(Box<Statement<R>>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Statement<R> where
  R: Clone + std::cmp::Eq,
{
    pub meta: Meta,
    pub content: RawStatement<R>,
}

#[derive(Debug, Clone, EnumIsA, EnumToGetters, EnumAsGetters, VariantName, VariantIndexArity)]
pub enum Switch<R> {
    /// Gives the `if` block and the `else` block
    If(Operand, Box<Statement<R>>, Box<Statement<R>>),
    /// Gives the integer type, a map linking values to switch branches, and the
    /// otherwise block. Note that matches over enumerations are performed by
    /// switching over the discriminant, which is an integer.
    /// Also, we use a `Vec` to make sure the order of the switch
    /// branches is preserved.
    ///
    /// Rk.: we use a vector of values, because some of the branches may
    /// be grouped together, like for the following code:
    /// ```text
    /// match e {
    ///   E::V1 | E::V2 => ..., // Grouped
    ///   E::V3 => ...
    /// }
    /// ```
    SwitchInt(
        Operand,
        IntegerTy,
        Vec<(Vec<ScalarValue>, Statement<R>)>,
        Box<Statement<R>>,
    ),
    /// A match over an ADT.
    ///
    /// The match statement is introduced in [crate::remove_read_discriminant]
    /// (whenever we find a discriminant read, we merge it with the subsequent
    /// switch into a match)
    Match(Place, Vec<(Vec<VariantId::Id>, Statement<R>)>, Box<Statement<R>>),
}

pub type ExprBody<R> = GExprBody<Statement<R>>;

pub type FunDecl<R> = GFunDecl<Statement<R>>;
pub type FunDecls<R> = FunDeclId::Vector<FunDecl<R>>;

pub type GlobalDecl<R> = GGlobalDecl<Statement<R>>;
pub type GlobalDecls<R> = GlobalDeclId::Vector<GlobalDecl<R>>;
