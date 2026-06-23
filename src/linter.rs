//! Linter for pyrst source code.
//!
//! Provides style checking, error detection, and best practice warnings.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Lint {
    // Diagnostic position pair, parallel to `code`/`message`. Construction sites
    // populate these (currently always 0) but no reader consumes them yet; kept
    // for when the linter reports source positions.
    #[allow(dead_code)]
    pub line: usize,
    #[allow(dead_code)]
    pub col: usize,
    pub level: LintLevel,
    pub code: String,
    pub message: String,
}

// The linter currently only emits `Warning`, but `driver.rs` matches on the
// full level taxonomy; preserve all three rather than narrowing the enum.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintLevel {
    Error,
    Warning,
    Info,
}

pub struct Linter {
    lints: Vec<Lint>,
    imported_names: HashSet<String>,
    used_names: HashSet<String>,
    defined_names: HashMap<String, usize>,
    imported_at: HashMap<String, usize>,
    in_function: bool,
    local_vars: HashSet<String>,
    local_used_vars: HashSet<String>,
}

impl Linter {
    fn new() -> Self {
        Self {
            lints: Vec::new(),
            imported_names: HashSet::new(),
            used_names: HashSet::new(),
            defined_names: HashMap::new(),
            imported_at: HashMap::new(),
            in_function: false,
            local_vars: HashSet::new(),
            local_used_vars: HashSet::new(),
        }
    }

    fn check_module(&mut self, m: &Module) {
        for stmt in &m.stmts {
            self.check_stmt(stmt);
        }
        self.check_unused_imports();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Func(f) => {
                // Check function naming convention (snake_case)
                if !is_snake_case(&f.name) {
                    self.lints.push(Lint {
                        line: 0,
                        col: 0,
                        level: LintLevel::Warning,
                        code: "W001".to_string(),
                        message: format!("function name '{}' should be snake_case", f.name),
                    });
                }

                // Check function length
                if f.body.len() > 50 {
                    self.lints.push(Lint {
                        line: 0,
                        col: 0,
                        level: LintLevel::Warning,
                        code: "W002".to_string(),
                        message: format!("function '{}' is too long ({} lines)", f.name, f.body.len()),
                    });
                }

                // Check parameter count
                if f.params.len() > 5 {
                    self.lints.push(Lint {
                        line: 0,
                        col: 0,
                        level: LintLevel::Warning,
                        code: "W003".to_string(),
                        message: format!(
                            "function '{}' has too many parameters ({})",
                            f.name,
                            f.params.len()
                        ),
                    });
                }

                // Track defined names
                self.defined_names.insert(f.name.clone(), 0);

                // Check body for uses (track local variables)
                let was_in_function = self.in_function;
                let saved_local_vars = self.local_vars.clone();
                let saved_local_used = self.local_used_vars.clone();

                self.in_function = true;
                self.local_vars.clear();
                self.local_used_vars.clear();

                // Add parameters to local variables
                for param in &f.params {
                    self.local_vars.insert(param.name.clone());
                }

                for s in &f.body {
                    self.check_stmt(s);
                }

                // Check for unused local variables
                for var in &self.local_vars {
                    if !self.local_used_vars.contains(var) && var != "self" {
                        self.lints.push(Lint {
                            line: 0,
                            col: 0,
                            level: LintLevel::Warning,
                            code: "W006".to_string(),
                            message: format!("unused variable: '{}'", var),
                        });
                    }
                }

                self.in_function = was_in_function;
                self.local_vars = saved_local_vars;
                self.local_used_vars = saved_local_used;
            }
            Stmt::Class(c) => {
                // Check class naming convention (CamelCase)
                if !is_pascal_case(&c.name) {
                    self.lints.push(Lint {
                        line: 0,
                        col: 0,
                        level: LintLevel::Warning,
                        code: "W004".to_string(),
                        message: format!("class name '{}' should be CamelCase", c.name),
                    });
                }

                self.defined_names.insert(c.name.clone(), 0);

                for m in &c.methods {
                    if !is_snake_case(&m.name) && m.name != "__init__" && m.name != "__str__" && m.name != "__eq__" && m.name != "__add__" {
                        self.lints.push(Lint {
                            line: 0,
                            col: 0,
                            level: LintLevel::Warning,
                            code: "W001".to_string(),
                            message: format!("method name '{}' should be snake_case", m.name),
                        });
                    }

                    // Track local variables in method
                    let was_in_function = self.in_function;
                    let saved_local_vars = self.local_vars.clone();
                    let saved_local_used = self.local_used_vars.clone();

                    self.in_function = true;
                    self.local_vars.clear();
                    self.local_used_vars.clear();

                    // Add parameters (including self) to local variables
                    for param in &m.params {
                        self.local_vars.insert(param.name.clone());
                    }

                    for s in &m.body {
                        self.check_stmt(s);
                    }

                    // Check for unused local variables in method
                    for var in &self.local_vars {
                        if !self.local_used_vars.contains(var) && var != "self" {
                            self.lints.push(Lint {
                                line: 0,
                                col: 0,
                                level: LintLevel::Warning,
                                code: "W006".to_string(),
                                message: format!("unused variable: '{}'", var),
                            });
                        }
                    }

                    self.in_function = was_in_function;
                    self.local_vars = saved_local_vars;
                    self.local_used_vars = saved_local_used;
                }
            }
            Stmt::Import { path, names, .. } => {
                let mod_name = path.join(".");
                if names.is_empty() {
                    self.imported_names.insert(mod_name.clone());
                    self.imported_at.insert(mod_name, 0);
                } else {
                    for (name, alias) in names {
                        let imported_as = alias.as_ref().unwrap_or(name).clone();
                        self.imported_names.insert(imported_as.clone());
                        self.imported_at.insert(imported_as, 0);
                    }
                }
            }
            Stmt::Assign { target, value, .. } => {
                if self.in_function {
                    self.local_vars.insert(target.clone());
                } else {
                    self.defined_names.insert(target.clone(), 0);
                }
                self.check_expr(value);
            }
            Stmt::Unpack { targets, value, .. } => {
                // Tuple unpacking: mark the unpacked tuple as used
                for target in targets {
                    if self.in_function {
                        self.local_vars.insert(target.clone());
                    } else {
                        self.defined_names.insert(target.clone(), 0);
                    }
                }
                self.check_expr(value);
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                self.check_expr(cond);
                for s in then {
                    self.check_stmt(s);
                }
                for (c, b) in elifs {
                    self.check_expr(c);
                    for s in b {
                        self.check_stmt(s);
                    }
                }
                if let Some(b) = else_ {
                    for s in b {
                        self.check_stmt(s);
                    }
                }
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond);
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::For { targets, iter, body, .. } => {
                for t in targets {
                    self.defined_names.insert(t.clone(), 0);
                }
                self.check_expr(iter);
                for s in body {
                    self.check_stmt(s);
                }
            }
            Stmt::Return(expr, _) => {
                if let Some(e) = expr {
                    self.check_expr(e);
                }
            }
            Stmt::Expr(e) => {
                self.check_expr(e);
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                for s in body {
                    self.check_stmt(s);
                }
                for h in handlers {
                    for s in &h.body {
                        self.check_stmt(s);
                    }
                }
                if let Some(b) = else_ {
                    for s in b {
                        self.check_stmt(s);
                    }
                }
                if let Some(b) = finally_ {
                    for s in b {
                        self.check_stmt(s);
                    }
                }
            }
            Stmt::AttrAssign { obj, value, .. } => {
                // Track usage of variables in the target base and the value.
                self.check_expr(obj);
                self.check_expr(value);
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                // Track usage of variables in the target base, index, and value.
                self.check_expr(obj);
                self.check_expr(idx);
                self.check_expr(value);
            }
            Stmt::AugAssign { target, value, .. } => {
                // Augmented assignment uses the target variable
                if self.in_function {
                    self.local_used_vars.insert(target.clone());
                } else {
                    self.used_names.insert(target.clone());
                }
                self.check_expr(value);
            }
            Stmt::Match { subject, arms, .. } => {
                self.check_expr(subject);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard);
                    }
                    for stmt in &arm.body {
                        self.check_stmt(stmt);
                    }
                }
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name, _) => {
                if self.in_function {
                    self.local_used_vars.insert(name.clone());
                } else {
                    self.used_names.insert(name.clone());
                }
            }
            Expr::Call { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(a);
                }
            }
            Expr::Attr { obj, .. } => {
                self.check_expr(obj);
            }
            Expr::Index { obj, idx, .. } => {
                self.check_expr(obj);
                self.check_expr(idx);
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                self.check_expr(obj);
                if let Some(e) = start { self.check_expr(e); }
                if let Some(e) = stop { self.check_expr(e); }
                if let Some(e) = step { self.check_expr(e); }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
            }
            Expr::UnOp { expr, .. } => {
                self.check_expr(expr);
            }
            Expr::List(elems, _) => {
                for e in elems {
                    self.check_expr(e);
                }
            }
            Expr::Tuple(elems, _) => {
                for e in elems {
                    self.check_expr(e);
                }
            }
            Expr::Set(elems, _) => {
                for e in elems {
                    self.check_expr(e);
                }
            }
            Expr::Dict(pairs, _) => {
                for (k, v) in pairs {
                    self.check_expr(k);
                    self.check_expr(v);
                }
            }
            Expr::ListComp { elt: _, iter, cond, .. } => {
                // Track usage of the iterator expression
                self.check_expr(iter);
                // Note: elt and cond use the loop variable which is local to the comprehension
                if let Some(c) = cond {
                    self.check_expr(c);
                }
                // We don't check elt because it contains the loop variable which is
                // scoped to the comprehension, but we should check iter
            }
            Expr::Lambda { params: _, body, .. } => {
                // Lambda parameters are local to the lambda; check the body
                // Variables referenced in the body that are lambda params aren't errors
                self.check_expr(body);
            }
            Expr::IfExp { test, body, orelse, .. } => {
                self.check_expr(test);
                self.check_expr(body);
                self.check_expr(orelse);
            }
            _ => {}
        }
    }

    fn check_unused_imports(&mut self) {
        for imported_name in &self.imported_names {
            if !self.used_names.contains(imported_name) {
                self.lints.push(Lint {
                    line: 0,
                    col: 0,
                    level: LintLevel::Warning,
                    code: "W005".to_string(),
                    message: format!("unused import: '{}'", imported_name),
                });
            }
        }
    }
}

fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Allow single letter names and dunder methods
    if s.len() == 1 {
        return s.chars().all(|c| c.is_lowercase() || c == '_');
    }
    if s.starts_with("__") && s.ends_with("__") {
        return true;
    }
    // Check if it's snake_case
    s.chars().all(|c| c.is_lowercase() || c == '_' || c.is_numeric()) && !s.starts_with('_')
}

fn is_pascal_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // First char must be uppercase
    if !s.chars().next().unwrap().is_uppercase() {
        return false;
    }
    // Rest can be alphanumeric
    s.chars().all(|c| c.is_alphanumeric())
}

pub fn lint(m: &Module) -> Vec<Lint> {
    let mut linter = Linter::new();
    linter.check_module(m);
    linter.lints
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse `src` and run the linter, returning the lint list.
    fn lint_src(src: &str) -> Vec<Lint> {
        let m = crate::parser::parse(src).expect("test source must parse");
        lint(&m)
    }

    /// Collect just the codes from the lint list.
    fn codes(lints: &[Lint]) -> Vec<&str> {
        lints.iter().map(|l| l.code.as_str()).collect()
    }

    /// Return lints whose code matches `code`.
    fn with_code<'a>(lints: &'a [Lint], code: &str) -> Vec<&'a Lint> {
        lints.iter().filter(|l| l.code == code).collect()
    }

    // ── W001: function/method name not snake_case ─────────────────────────────

    #[test]
    fn w001_camel_case_function_triggers() {
        let src = "def myFunction(x: int) -> int:\n    return x\n";
        let lints = lint_src(src);
        let w001: Vec<_> = with_code(&lints, "W001");
        assert!(!w001.is_empty(), "expected W001 for camelCase function name");
        assert!(
            w001[0].message.contains("myFunction"),
            "message must name the offending function"
        );
    }

    #[test]
    fn w001_snake_case_function_clean() {
        let src = "def my_function(x: int) -> int:\n    return x\n";
        let lints = lint_src(src);
        assert!(
            with_code(&lints, "W001").is_empty(),
            "snake_case function should not trigger W001"
        );
    }

    #[test]
    fn w001_dunder_method_exempt() {
        // __init__, __str__, __eq__, __add__ must NOT trigger W001 inside a class.
        let src = "class Foo:\n    def __init__(self) -> None:\n        pass\n";
        let lints = lint_src(src);
        assert!(
            with_code(&lints, "W001").is_empty(),
            "__init__ must be exempt from W001"
        );
    }

    #[test]
    fn w001_method_bad_name_triggers() {
        // A method that isn't a dunder and uses camelCase should trigger W001.
        let src = "class Foo:\n    def myMethod(self) -> None:\n        pass\n";
        let lints = lint_src(src);
        let w001 = with_code(&lints, "W001");
        assert!(!w001.is_empty(), "camelCase method must trigger W001");
        assert!(w001[0].message.contains("myMethod"));
    }

    // ── W002: function body > 50 statements ──────────────────────────────────

    #[test]
    fn w002_short_function_clean() {
        // A 3-statement body is well under the 50-stmt threshold.
        let body = "    x: int = 1\n    y: int = 2\n    return x\n";
        let src = format!("def short_fn(x: int) -> int:\n{}", body);
        let lints = lint_src(&src);
        assert!(with_code(&lints, "W002").is_empty(), "short function must not trigger W002");
    }

    #[test]
    fn w002_long_function_triggers() {
        // Build a function with 51 pass statements (> 50 → W002).
        let stmts = "    pass\n".repeat(51);
        let src = format!("def long_fn() -> None:\n{}", stmts);
        let lints = lint_src(&src);
        let w002 = with_code(&lints, "W002");
        assert!(!w002.is_empty(), "51-stmt function must trigger W002");
        assert!(w002[0].message.contains("long_fn"));
    }

    // ── W003: too many parameters (> 5) ──────────────────────────────────────

    #[test]
    fn w003_five_params_clean() {
        // Exactly 5 parameters: no W003.
        let src = "def f(a: int, b: int, c: int, d: int, e: int) -> int:\n    return a\n";
        let lints = lint_src(src);
        assert!(with_code(&lints, "W003").is_empty(), "5 params must not trigger W003");
    }

    #[test]
    fn w003_six_params_triggers() {
        // 6 parameters exceeds the limit of 5 → W003.
        let src =
            "def f(a: int, b: int, c: int, d: int, e: int, g: int) -> int:\n    return a\n";
        let lints = lint_src(src);
        let w003 = with_code(&lints, "W003");
        assert!(!w003.is_empty(), "6 params must trigger W003");
        assert!(w003[0].message.contains("f"));
    }

    // ── W004: class name not PascalCase ──────────────────────────────────────

    #[test]
    fn w004_snake_class_name_triggers() {
        let src = "class my_class:\n    pass\n";
        let lints = lint_src(src);
        let w004 = with_code(&lints, "W004");
        assert!(!w004.is_empty(), "snake_case class name must trigger W004");
        assert!(w004[0].message.contains("my_class"));
    }

    #[test]
    fn w004_pascal_class_name_clean() {
        let src = "class MyClass:\n    pass\n";
        let lints = lint_src(src);
        assert!(with_code(&lints, "W004").is_empty(), "PascalCase class name must not trigger W004");
    }

    // ── W005: unused import ───────────────────────────────────────────────────

    #[test]
    fn w005_unused_import_triggers() {
        // Import `math` but never reference it → W005.
        let src = "import math\nx: int = 1\n";
        let lints = lint_src(src);
        let w005 = with_code(&lints, "W005");
        assert!(!w005.is_empty(), "unused import must trigger W005");
        assert!(w005[0].message.contains("math"));
    }

    #[test]
    fn w005_used_import_clean() {
        // Use a from-import and reference the imported name directly.
        let src2 = "from os import path\nx: str = path\n";
        let lints = lint_src(src2);
        assert!(
            with_code(&lints, "W005").is_empty(),
            "used import must not trigger W005"
        );
    }

    // ── W006: unused local variable ───────────────────────────────────────────

    #[test]
    fn w006_unused_param_triggers() {
        // Parameter `unused` is never referenced in the body → W006.
        let src = "def f(used: int, unused: int) -> int:\n    return used\n";
        let lints = lint_src(src);
        let w006 = with_code(&lints, "W006");
        assert!(!w006.is_empty(), "unused param must trigger W006");
        assert!(w006[0].message.contains("unused"));
    }

    #[test]
    fn w006_all_params_used_clean() {
        let src = "def f(a: int, b: int) -> int:\n    return a + b\n";
        let lints = lint_src(src);
        assert!(with_code(&lints, "W006").is_empty(), "all-used params must not trigger W006");
    }

    #[test]
    fn w006_self_param_exempt() {
        // `self` is always implicitly used; must not appear in W006 messages.
        let src = "class Foo:\n    def bar(self) -> None:\n        pass\n";
        let lints = lint_src(src);
        let w006 = with_code(&lints, "W006");
        // If there are any W006 lints they must not be about `self`.
        for lint in &w006 {
            assert!(!lint.message.contains("'self'"), "self must be exempt from W006");
        }
    }

    #[test]
    fn w006_unused_local_assign_triggers() {
        // Assign to `z` inside a function but never read it → W006.
        let src = "def f(x: int) -> int:\n    z: int = 42\n    return x\n";
        let lints = lint_src(src);
        let w006 = with_code(&lints, "W006");
        assert!(!w006.is_empty(), "unused local variable must trigger W006");
        assert!(w006.iter().any(|l| l.message.contains("'z'")));
    }

    // ── Lint level ────────────────────────────────────────────────────────────

    #[test]
    fn all_emitted_lints_are_warnings() {
        // Every lint the linter currently produces must have level Warning.
        let src = "def myFunc(a: int, b: int, c: int, d: int, e: int, f: int) -> None:\n    pass\n";
        let lints = lint_src(src);
        for l in &lints {
            assert_eq!(
                l.level,
                LintLevel::Warning,
                "expected Warning level, got {:?} for code {}",
                l.level,
                l.code
            );
        }
    }

    // ── Multiple rules fire together ──────────────────────────────────────────

    #[test]
    fn multiple_rules_can_fire_simultaneously() {
        // camelCase name (W001) + 6 params (W003) — both must fire.
        let src =
            "def badFunc(a: int, b: int, c: int, d: int, e: int, f: int) -> None:\n    pass\n";
        let lints = lint_src(src);
        let c = codes(&lints);
        assert!(c.contains(&"W001"), "W001 must fire");
        assert!(c.contains(&"W003"), "W003 must fire");
    }
}
