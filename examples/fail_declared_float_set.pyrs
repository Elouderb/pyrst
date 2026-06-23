# A DECLARED `set[float]` annotation resolves to Set(Float) and would emit the
# uncompilable `HashSet<f64>` even with an empty `set()` initializer. The type
# checker rejects float sets at the TypeExpr->Ty resolver, so declared types are
# covered uniformly (vars, params, returns) — not just literals (card 3c0243de).
def main() -> None:
    s: set[float] = set()  # declared float set — not hashable
    print(len(s))
