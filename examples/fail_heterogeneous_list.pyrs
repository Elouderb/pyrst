# A list literal mixing genuinely-incompatible concrete element types (int vs
# str) must be rejected at the type checker, not silently typed as List(int)
# and deferred to rustc.
def main() -> None:
    xs = [1, "hello"]  # int vs str
    print(len(xs))
