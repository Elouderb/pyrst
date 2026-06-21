# EPIC-5: None ~ Option, bare T -> Some, `return None` for an Optional return.
def lookup(found: bool) -> Optional[int]:
    if found:
        return 42
    return None


def main() -> None:
    hit: Optional[int] = lookup(True)
    miss: Optional[int] = lookup(False)
    empty: Optional[int] = None
    print(hit is None)
    print(miss is None)
    print(empty is None)
    if hit is not None:
        print(hit + 1)
