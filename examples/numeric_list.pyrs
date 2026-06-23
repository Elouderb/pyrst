# Mixed int/float list literals widen to list[float] end-to-end:
# typeck unifies the element types to Float and codegen casts the int
# literals to f64 so the emitted Vec<f64> is homogeneous and compiles
# (card 5c2f31d8). NOTE: a numeric *set* literal is intentionally not
# exercised here — a set[float] (HashSet<f64>) is not representable in
# Rust today (f64 is not Eq/Hash), so this example is list-only.

def main() -> None:
    # Int and float literals mixed; trailing int after the float too.
    xs = [1, 2.0, 3]
    print(xs)
    print(len(xs))

    total = 0.0
    for x in xs:
        total += x
    print(total)

    # Float-first ordering also widens to float.
    ys = [1.5, 2, 4]
    print(ys)
    print(sum(ys))

main()
