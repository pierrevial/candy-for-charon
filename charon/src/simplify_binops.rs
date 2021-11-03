//! In MIR, many binops are desugared into:
//! * for division/modulo: a test that the divider is non zero (making the code
//!   panics if the divider is zero), then the division itself
//! * an operation, followed by a test: typically an addition followed by a check
//!   for overflow
//! This is a bit too low-level for us: we only want to have the binop (which will
//! have a precondition in our theorem prover, or will be monadic...). We thus want
//! to remove those unnecessary checks.

use crate::cfim_ast::*;
use crate::expressions::*;
use crate::types::*;
use crate::values::*;
use hashlink::linked_hash_map::LinkedHashMap;
use std::iter::FromIterator;

/// Return true iff: `place ++ [pelem] == full_place`
fn check_places_similar_but_last_proj_elem(
    place: &Place,
    pelem: &ProjectionElem,
    full_place: &Place,
) -> bool {
    if place.var_id == full_place.var_id
        && place.projection.len() + 1 == full_place.projection.len()
    {
        for i in 0..place.projection.len() {
            if place.projection[i] != full_place.projection[i] {
                return false;
            }
        }

        return *pelem == full_place.projection[place.projection.len()];
    }
    return false;
}

/// Return true if the binary operation might fail and thus requires its result
/// to be checked (overflows, for instance).
fn binop_requires_assert_after(binop: BinOp) -> bool {
    match binop {
        BinOp::BitXor
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::Eq
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Ne
        | BinOp::Ge
        | BinOp::Gt
        | BinOp::Div
        | BinOp::Rem => false,
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl | BinOp::Shr => true,
    }
}

/// Return true if the binary operation has a precondition (divisor is non zero
/// for instance) and must thus be preceded by an assertion.
fn binop_requires_assert_before(binop: BinOp) -> bool {
    match binop {
        BinOp::BitXor
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::Eq
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Ne
        | BinOp::Ge
        | BinOp::Gt
        | BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Shl
        | BinOp::Shr => false,
        BinOp::Div | BinOp::Rem => true,
    }
}

fn binop_can_fail(binop: BinOp) -> bool {
    binop_requires_assert_after(binop) || binop_requires_assert_before(binop)
}

/// Check if this is a group of expressions of the form: "do an operation,
/// then check it succeeded (didn't overflow, etc.)".
///   ```
///   ```
/// Check if this is a group of expressions which should be collapsed to a
/// single checked binop.
/// Simply check if the first expression is a checked binop.
fn check_if_binop_then_assert(exp1: &Expression, exp2: &Expression, exp3: &Expression) -> bool {
    match exp1 {
        Expression::Statement(Statement::Assign(_, Rvalue::BinaryOp(binop, _, _))) => {
            if binop_requires_assert_after(*binop) {
                // We found a checked binary op.
                // Make sure this group of expressions should exactly match the
                // following pattern:
                //   ```
                //   tmp := copy x + copy y; // Possibly a different binop
                //   assert(move (tmp.1) == false);
                //   dest := move (tmp.0);
                //   ...
                //   ```
                // If it is note the case, we can't collapse...
                check_if_simplifiable_binop_then_assert(exp1, exp2, exp3);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Make sure the expressions match the following pattern:
///   ```
///   tmp := copy x + copy y; // Possibly a different binop
///   assert(move (tmp.1) == false);
///   dest := move (tmp.0);
///   ...
///   ```
fn check_if_simplifiable_binop_then_assert(
    exp1: &Expression,
    exp2: &Expression,
    exp3: &Expression,
) {
    match (exp1, exp2, exp3) {
        (
            Expression::Statement(Statement::Assign(bp, Rvalue::BinaryOp(binop, _op1, _op2))),
            Expression::Statement(Statement::Assert(Assert {
                cond: Operand::Move(cond_op),
                expected,
            })),
            Expression::Statement(Statement::Assign(_mp, Rvalue::Use(Operand::Move(mr)))),
        ) => {
            assert!(binop_requires_assert_after(*binop));
            assert!(!(*expected));

            // We must have:
            // cond_op == bp.1
            // mr == bp.0
            let check1 = check_places_similar_but_last_proj_elem(
                bp,
                &ProjectionElem::Field(FieldProjKind::Tuple(2), FieldId::Id::new(1)),
                cond_op,
            );
            assert!(check1);

            let check2 = check_places_similar_but_last_proj_elem(
                bp,
                &ProjectionElem::Field(FieldProjKind::Tuple(2), FieldId::Id::new(0)),
                mr,
            );
            assert!(check2);
        }
        _ => {
            unreachable!();
        }
    }
}

/// Simplify patterns of the form:
///   ```
///   tmp := copy x + copy y; // Possibly a different binop
///   assert(move (tmp.1) == false);
///   dest := move (tmp.0);
///   ...
///   ```
/// to:
///   ```
///   tmp := copy x + copy y; // Possibly a different binop
///   ...
///   ```
/// Note that the type of the binop changes in the two situations (in the
/// translation, before the transformation `+` returns a pair (bool, int),
/// after it has a monadic type).
fn simplify_binop_then_assert(exp1: Expression, exp2: Expression, exp3: Expression) -> Expression {
    match (exp1, exp2, exp3) {
        (
            Expression::Statement(Statement::Assign(_, binop)),
            Expression::Statement(Statement::Assert(_)),
            Expression::Statement(Statement::Assign(mp, _)),
        ) => {
            return Expression::Statement(Statement::Assign(mp, binop));
        }
        _ => {
            unreachable!();
        }
    }
}

/// Check if this is a group of expressions of the form: "check that we can do
/// an binary operation, then do this operation (ex.: check that a divisor is
/// non zero before doing a division, panic otherwise)"
fn check_if_assert_then_binop(exp1: &Expression, exp2: &Expression, exp3: &Expression) -> bool {
    match exp3 {
        Expression::Statement(Statement::Assign(_, Rvalue::BinaryOp(binop, _, _))) => {
            if binop_requires_assert_before(*binop) {
                // We found an unchecked binop which should be simplified (division
                // or remainder computation).
                // Make sure the group of expressions exactly matches the following
                // pattern:
                //   ```
                //   tmp := (copy divisor) == 0;
                //   assert((move tmp) == false);
                //   dest := move dividend / move divisor; // Can also be a `%`
                //   ...
                //   ```
                // If it is note the case, we can't collapse...
                check_if_simplifiable_assert_then_binop(exp1, exp2, exp3);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Make sure the expressions match the following pattern:
///   ```
///   tmp := (copy divisor) == 0;
///   assert((move tmp) == false);
///   dest := move dividend / move divisor; // Can also be a `%`
///   ...
///   ```
fn check_if_simplifiable_assert_then_binop(
    exp1: &Expression,
    exp2: &Expression,
    exp3: &Expression,
) {
    match (exp1, exp2, exp3) {
        (
            Expression::Statement(Statement::Assign(
                eq_dest,
                Rvalue::BinaryOp(
                    BinOp::Eq,
                    Operand::Copy(eq_op1),
                    Operand::Constant(
                        _,
                        OperandConstantValue::ConstantValue(ConstantValue::Scalar(scalar_value)),
                    ),
                ),
            )),
            Expression::Statement(Statement::Assert(Assert {
                cond: Operand::Move(cond_op),
                expected,
            })),
            Expression::Statement(Statement::Assign(
                _mp,
                Rvalue::BinaryOp(binop, _dividend, Operand::Move(divisor)),
            )),
        ) => {
            assert!(binop_requires_assert_before(*binop));
            assert!(!(*expected));
            assert!(eq_op1 == divisor);
            assert!(eq_dest == cond_op);
            if scalar_value.is_int() {
                assert!(scalar_value.as_int().unwrap() == 0);
            } else {
                assert!(scalar_value.as_uint().unwrap() == 0);
            }
        }
        _ => {
            unreachable!();
        }
    }
}

/// Simplify patterns of the form:
///   ```
///   tmp := (copy divisor) == 0;
///   assert((move tmp) == false);
///   dest := move dividend / move divisor; // Can also be a `%`
///   ...
///   ```
/// to:
///   ```
///   dest := move dividend / move divisor; // Can also be a `%`
///   ...
///   ```
fn simplify_assert_then_binop(
    _exp1: Expression,
    _exp2: Expression,
    exp3: Expression,
) -> Expression {
    exp3
}

/// Check if the statement is an assignment which uses a binop which can fail
/// (it is a checked binop, or a binop with a precondition like division)
fn statement_is_faillible_binop(st: &Statement) -> bool {
    match st {
        Statement::Assign(_, Rvalue::BinaryOp(binop, _, _)) => binop_can_fail(*binop),
        _ => false,
    }
}

fn simplify_exp(exp: Expression) -> Expression {
    match exp {
        Expression::Statement(st) => {
            // Check that we never failed to simplify a binop
            assert!(!statement_is_faillible_binop(&st));
            Expression::Statement(st)
        }
        Expression::Switch(op, targets) => {
            let targets = match targets {
                SwitchTargets::If(exp1, exp2) => {
                    SwitchTargets::If(Box::new(simplify_exp(*exp1)), Box::new(simplify_exp(*exp2)))
                }
                SwitchTargets::SwitchInt(int_ty, targets, otherwise) => {
                    let targets = LinkedHashMap::from_iter(
                        targets.into_iter().map(|(v, e)| (v, simplify_exp(e))),
                    );
                    let otherwise = simplify_exp(*otherwise);
                    SwitchTargets::SwitchInt(int_ty, targets, Box::new(otherwise))
                }
            };
            Expression::Switch(op, targets)
        }
        Expression::Loop(loop_body) => Expression::Loop(Box::new(simplify_exp(*loop_body))),
        Expression::Sequence(exp1, exp2) => match *exp2 {
            Expression::Sequence(exp2, exp3) => {
                match *exp3 {
                    Expression::Sequence(exp3, exp4) => {
                        // Simplify checked binops
                        if check_if_binop_then_assert(&exp1, &exp2, &exp3) {
                            let exp = simplify_binop_then_assert(*exp1, *exp2, *exp3);
                            let exp4 = simplify_exp(*exp4);
                            return Expression::Sequence(Box::new(exp), Box::new(exp4));
                        }
                        // Simplify unchecked binops (division, modulo)
                        if check_if_assert_then_binop(&exp1, &exp2, &exp3) {
                            let exp = simplify_assert_then_binop(*exp1, *exp2, *exp3);
                            let exp4 = simplify_exp(*exp4);
                            return Expression::Sequence(Box::new(exp), Box::new(exp4));
                        }
                        // Not simplifyable
                        else {
                            let next_exp = Expression::Sequence(
                                exp2,
                                Box::new(Expression::Sequence(exp3, exp4)),
                            );
                            Expression::Sequence(
                                Box::new(simplify_exp(*exp1)),
                                Box::new(simplify_exp(next_exp)),
                            )
                        }
                    }
                    exp3 => Expression::Sequence(
                        Box::new(simplify_exp(*exp1)),
                        Box::new(simplify_exp(Expression::Sequence(exp2, Box::new(exp3)))),
                    ),
                }
            }
            exp2 => {
                Expression::Sequence(Box::new(simplify_exp(*exp1)), Box::new(simplify_exp(exp2)))
            }
        },
    }
}

fn simplify_def(mut def: FunDef) -> FunDef {
    trace!("About to simplify: {}", def.name);
    def.body = simplify_exp(def.body);
    def
}

pub fn simplify(defs: FunDefs) -> FunDefs {
    FunDefs::from_iter(defs.into_iter().map(|def| simplify_def(def)))
}
