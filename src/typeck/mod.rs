//! v0 type checker with full function body type checking, name resolution, and arity checking.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{Error, Result, Span};

mod types;
mod checks;
mod generics;
mod flow;
mod exprs;

pub(crate) use types::*;
pub(crate) use checks::*;
pub(crate) use generics::*;
pub(crate) use flow::*;
pub(crate) use exprs::*;

// Architecture: one in-file #[cfg(test)] block at the bottom of the module so
// private items (types_compatible, check_expr, check_stmt, FuncEnv) are
// accessible without pub-widening any production code.
//
// Four categories:
//   A. types_compatible matrix         (~19 cases)
//   B. builtin_method_ret              (~20 cases)
//   C. inference via check_expr/stmt   (~24 cases)
//   D. error-firing                    (~13 cases)
//
// CHARACTERIZATION philosophy: each test asserts the code's ACTUAL current
// behaviour. Where behaviour is a known limitation or design choice, a comment
// marks it (BUG 1, BUG 2, BUG 3).
// =============================================================================
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_a;
#[cfg(test)]
mod tests_b;
