# A float-keyed dict literal `{1.0: "a"}` types as Dict(Float, _), which codegen
# would emit as `HashMap<f64, _>` — uncompilable in Rust because f64 is not
# Eq/Hash. The type checker must reject the float KEY rather than defer to rustc
# (card 3c0243de). Float VALUES are fine; only float keys are rejected.
def main() -> None:
    d = {1.0: "a"}  # float dict key — not hashable
    print(len(d))
