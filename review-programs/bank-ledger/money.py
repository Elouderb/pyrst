# money.py -- integer-cents money helpers for the ledger.
#
# Money is modeled as an integer number of *cents* (i64). This avoids floating
# point rounding error in balances and keeps output deterministic. All public
# amounts crossing the bank API are cents.

# Render a signed cents amount as a dollar string, e.g. 123456 -> "$1234.56",
# -55 -> "-$0.55". Used by reports and __str__ methods.
def format_cents(cents: int) -> str:
    sign: str = ""
    n: int = cents
    if n < 0:
        sign = "-"
        n = -n
    dollars: int = n // 100
    remainder: int = n % 100
    # zero-pad the cents to two digits via f-string format spec.
    return f"{sign}${dollars}.{remainder:02d}"

# Convert a whole-dollar count to cents.
def dollars(whole: int) -> int:
    return whole * 100

# Apply a basis-point rate to a principal, returning the interest in cents,
# truncated toward zero (floor division on non-negative principal). 100 bps = 1%.
def interest_cents(principal_cents: int, rate_bps: int) -> int:
    if principal_cents <= 0:
        return 0
    return (principal_cents * rate_bps) // 10000

# Clamp a value into [lo, hi]; used to keep tier indices in range.
def clamp(value: int, lo: int, hi: int) -> int:
    if value < lo:
        return lo
    if value > hi:
        return hi
    return value
