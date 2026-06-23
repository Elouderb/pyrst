# Negative test: range() now types as list[int], so passing it where a str is
# expected is caught by pyrst's checker (previously deferred to rustc).
def expect_str(x: str) -> None:
    print(x)

def main() -> None:
    r = range(5)
    expect_str(r)
