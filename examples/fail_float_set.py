# A pure-float set literal `{1.0, 2.0}` types as Set(Float), which codegen would
# emit as `HashSet<f64>` — uncompilable in Rust because f64 is not Eq/Hash. The
# type checker must reject it rather than defer to rustc (card 3c0243de).
def main() -> None:
    xs = {1.0, 2.0}  # float set element — not hashable
    print(len(xs))
