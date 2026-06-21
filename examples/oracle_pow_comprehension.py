# D5: Pow -> Float in list comprehension element position.
# After E.4/E.5 the inferred element type for [x**2 for x in [1,2,3]] is
# Float (Vec<f64>), and the emitted i64 power value is coerced correctly so
# the rustc output type matches.  Proves inference + emission agree for **
# inside comprehension element expressions.
def main() -> None:
    result: list[float] = [x ** 2 for x in [1, 2, 3]]
    print(result)          # [1.0, 4.0, 9.0]
