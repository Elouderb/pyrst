# Negative test: string iteration now binds str element type, so a char passed
# where an int is expected is caught at `pyrst check`.
def expect_int_arg(x: int) -> None:
    print(x)

def main() -> None:
    text: str = "hello"
    for c in text:
        expect_int_arg(c)
